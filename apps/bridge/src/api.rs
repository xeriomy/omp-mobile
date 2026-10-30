//! Versioned HTTP API and authenticated WebSocket event stream.

use crate::api_types::{
    API_VERSION, BRIDGE_VERSION, BridgeEvent, EVENT_VERSION, ErrorBody, ErrorEnvelope,
    HealthResponse, PairRequest, PairResponse, StatusResponse, WebSocketInfo,
};
use crate::auth::{AuthStore, PairError};
use crate::config::BridgeConfig;
use crate::engine::Engine;
use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_MAX_AGE, AUTHORIZATION, HOST, ORIGIN, VARY,
};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

const WS_AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const WS_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST_BODY: usize = 16 * 1024;
const CORS_METHODS: &str = "GET, POST, OPTIONS";
const CORS_HEADERS: &str = "authorization, content-type";
const WS_CLOSE_UNAUTHORIZED: u16 = 4401;
const WS_CLOSE_TIMEOUT: u16 = 4408;
const WS_CLOSE_INTERNAL: u16 = 1011;

#[derive(Clone)]
pub struct AppState {
    pub engine: Engine,
    pub auth: AuthStore,
    pub config: Arc<BridgeConfig>,
    pub listen: SocketAddr,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/api/v1/pair", post(pair))
        .route("/api/v1/status", get(status))
        .route("/api/v1/tokens/revoke", post(revoke))
        .route("/api/v1/events", get(websocket_upgrade))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            origin_and_host_guard,
        ))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse::ok())
}

async fn pair(
    State(state): State<AppState>,
    body: Result<Json<PairRequest>, JsonRejection>,
) -> Result<Json<PairResponse>, ApiError> {
    let Json(request) = body.map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "Request body must be JSON with a string code field.",
        )
    })?;
    let issued = state
        .auth
        .pair(&request.code)
        .map_err(|error| match error {
            PairError::Invalid => ApiError::new(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Pairing code is invalid or was already used.",
            ),
            PairError::Expired => ApiError::new(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Pairing code expired. Restart the bridge to create a new code.",
            ),
            PairError::Locked => ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                "Too many pairing attempts. Restart the bridge to create a new code.",
            ),
        })?;
    Ok(Json(PairResponse {
        token: issued.token,
        token_id: issued.token_id,
        expires_at: issued.expires_at,
    }))
}

async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<StatusResponse>, ApiError> {
    authorize(&state, &headers)?;
    Ok(Json(status_payload(&state)))
}

async fn revoke(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, ApiError> {
    let token = bearer_token(&headers)?;
    if !state.auth.revoke(token) {
        return Err(ApiError::unauthorized());
    }
    Ok(StatusCode::NO_CONTENT)
}

fn status_payload(state: &AppState) -> StatusResponse {
    StatusResponse {
        api_version: API_VERSION.to_owned(),
        bridge_version: BRIDGE_VERSION.to_owned(),
        listen: state.listen.to_string(),
        engine: state.engine.status(),
        websocket: WebSocketInfo {
            path: "/api/v1/events".to_owned(),
            auth: "first-message".to_owned(),
            event_version: EVENT_VERSION,
        },
    }
}

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<String, ApiError> {
    let token = bearer_token(headers)?;
    state
        .auth
        .authorize(token)
        .ok_or_else(ApiError::unauthorized)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(ApiError::unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
        .ok_or_else(ApiError::unauthorized)?;
    Ok(token)
}

async fn websocket_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| websocket_session(socket, state))
}

async fn websocket_session(mut socket: WebSocket, state: AppState) {
    let auth = timeout(WS_AUTH_TIMEOUT, socket.recv()).await;
    let token = match auth {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(|value| value.get("type").and_then(Value::as_str) == Some("authenticate"))
            .and_then(|value| {
                value
                    .get("token")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
            return;
        }
        Ok(Some(Ok(_))) => None,
        Ok(Some(Err(_))) => return,
        Err(_) => {
            let _ = send_close(&mut socket, WS_CLOSE_TIMEOUT, "authentication timed out").await;
            return;
        }
    };

    let Some(token) = token else {
        let _ = send_close(&mut socket, WS_CLOSE_UNAUTHORIZED, "unauthorized").await;
        return;
    };
    if state.auth.authorize(&token).is_none() {
        let _ = send_close(&mut socket, WS_CLOSE_UNAUTHORIZED, "unauthorized").await;
        return;
    }

    let mut events = state.engine.subscribe();
    let initial = state.engine.make_event(
        "bridge.status",
        serde_json::to_value(status_payload(&state)).unwrap_or(Value::Null),
    );
    if send_event(&mut socket, &initial).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        if send_event(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        let resync = state.engine.make_event("bridge.resync", json!({
                            "skipped": skipped,
                            "status": status_payload(&state),
                        }));
                        if send_event(&mut socket, &resync).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    None | Some(Ok(Message::Close(_))) | Some(Err(_)) => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }
    let _ = send_close(&mut socket, WS_CLOSE_INTERNAL, "bridge event stream closed").await;
}

async fn send_event(socket: &mut WebSocket, event: &BridgeEvent) -> Result<(), ()> {
    let encoded = serde_json::to_string(event).map_err(|_| ())?;
    timeout(WS_SEND_TIMEOUT, socket.send(Message::Text(encoded.into())))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

async fn send_close(socket: &mut WebSocket, code: u16, reason: &str) -> Result<(), ()> {
    let frame = CloseFrame {
        code,
        reason: reason.into(),
    };
    timeout(WS_SEND_TIMEOUT, socket.send(Message::Close(Some(frame))))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

async fn origin_and_host_guard(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok());
    if !host
        .map(|host| host_matches(host, state.listen.port()))
        .unwrap_or(false)
    {
        return ApiError::new(
            StatusCode::BAD_REQUEST,
            "INVALID_HOST",
            "Host header must target the configured loopback listener.",
        )
        .into_response();
    }

    let origin = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if let Some(origin) = &origin
        && !state.config.origin_allowed(origin)
    {
        return with_cors(
            ApiError::new(
                StatusCode::FORBIDDEN,
                "INVALID_ORIGIN",
                "Origin is not in the bridge allowlist.",
            )
            .into_response(),
            None,
        );
    }

    if request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static(CORS_METHODS),
        );
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static(CORS_HEADERS),
        );
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
        return with_cors(response, origin.as_deref());
    }

    let response = next.run(request).await;
    with_cors(response, origin.as_deref())
}

fn with_cors(mut response: Response, origin: Option<&str>) -> Response {
    if let Some(origin) = origin
        && let Ok(value) = HeaderValue::from_str(origin)
    {
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_ALLOW_ORIGIN, value);
        response
            .headers_mut()
            .insert(VARY, HeaderValue::from_static("Origin"));
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static(CORS_HEADERS),
        );
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static(CORS_METHODS),
        );
    }
    response
}

fn host_matches(host: &str, port: u16) -> bool {
    let host = host.trim();
    let expected = ["127.0.0.1", "localhost", "[::1]"];
    if let Some((name, value)) = host.rsplit_once(':')
        && !value.is_empty()
        && value.chars().all(|ch| ch.is_ascii_digit())
    {
        return value == port.to_string() && expected.contains(&name);
    }
    expected.contains(&host) && port == 80
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "A valid bearer token is required.",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorEnvelope {
            error: ErrorBody {
                code: self.code.to_owned(),
                message: self.message,
                request_id: crate::auth::new_request_id(),
            },
        };
        (self.status, Json(body)).into_response()
    }
}
