/**
 * Typed client for the versioned OMP bridge API.
 *
 * The token stays in memory only. It is never placed in a URL, logged, or
 * written to localStorage: the bridge drops all tokens on restart, so durable
 * storage would be both unsafe and useless before secure storage exists.
 */

export type EnginePhase = 'starting' | 'ready' | 'failed' | 'exited';

export interface ModelSummary {
  provider: string;
  id: string;
}

export interface EngineLimits {
  maxFrameBytes: number;
  maxReassembledFrameBytes: number;
}

export interface ReadyInfo {
  protocolVersion: number;
  supportedProtocolVersions: number[];
  limits: EngineLimits;
}

export interface EngineErrorInfo {
  code: string;
  message: string;
}

export interface SafeStateSummary {
  model?: ModelSummary;
  thinkingLevel?: string;
  isStreaming?: boolean;
  isCompacting?: boolean;
  isSettled?: boolean;
  messageCount?: number;
  queuedMessageCount?: number;
  hasPendingAsyncWork?: boolean;
  sessionId?: string;
  sessionName?: string;
}

export interface EngineStatus {
  state: EnginePhase;
  ready: boolean;
  readyInfo?: ReadyInfo;
  negotiatedProtocolVersion?: number;
  summary?: SafeStateSummary;
  error?: EngineErrorInfo;
}

export interface StatusResponse {
  apiVersion: string;
  bridgeVersion: string;
  listen: string;
  engine: EngineStatus;
  websocket: { path: string; auth: string; eventVersion: number };
}

export interface BridgeEvent {
  version: number;
  type: string;
  sequence: number;
  occurredAt: number;
  replay?: boolean;
  payload: unknown;
}

export interface PairResponse {
  token: string;
  tokenId: string;
  expiresAt: number;
}

export class BridgeError extends Error {
  readonly code: string;
  readonly status?: number;
  readonly requestId?: string;

  constructor(message: string, code: string, status?: number, requestId?: string) {
    super(message);
    this.name = 'BridgeError';
    this.code = code;
    this.status = status;
    this.requestId = requestId;
  }
}

const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '[::1]', '::1']);

export interface BridgeEndpoints {
  baseUrl: string;
  eventUrl: string;
}

/** Accept only a bare loopback http(s) origin. */
export function normalizeBridgeUrl(input: string): BridgeEndpoints {
  const trimmed = input.trim();
  if (!trimmed) {
    throw new BridgeError('Enter the bridge URL printed by omp-bridge.', 'URL_REQUIRED');
  }
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw new BridgeError('Bridge URL is not a valid http:// address.', 'URL_INVALID');
  }
  if (url.protocol !== 'http:') {
    throw new BridgeError(
      'Bridge URL must use http://. The bridge does not serve TLS.',
      'URL_INVALID',
    );
  }
  if (url.username || url.password || url.search || url.hash) {
    throw new BridgeError(
      'Bridge URL must not contain credentials, a query, or a fragment.',
      'URL_INVALID',
    );
  }
  if (url.pathname !== '' && url.pathname !== '/') {
    throw new BridgeError('Use the bridge origin only, without a path.', 'URL_INVALID');
  }
  if (!LOOPBACK_HOSTS.has(url.hostname)) {
    throw new BridgeError(
      'This build connects to loopback only. Use 127.0.0.1 or localhost.',
      'URL_NOT_LOOPBACK',
    );
  }
  const baseUrl = url.origin;
  return {
    baseUrl,
    eventUrl: `${baseUrl.replace(/^http/, 'ws')}/api/v1/events`,
  };
}

async function request<T>(url: string, init: RequestInit, token?: string): Promise<T> {
  let response: Response;
  try {
    response = await fetch(url, {
      ...init,
      cache: 'no-store',
      headers: {
        ...(init.body ? { 'Content-Type': 'application/json' } : {}),
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...init.headers,
      },
    });
  } catch {
    throw new BridgeError(
      `Cannot reach the bridge at ${new URL(url).origin}. Start omp-bridge in Termux and keep it running.`,
      'NETWORK',
    );
  }
  if (response.status === 204) {
    return undefined as T;
  }
  const text = await response.text();
  let body: unknown = null;
  if (text) {
    try {
      body = JSON.parse(text);
    } catch {
      body = null;
    }
  }
  if (!response.ok) {
    const error = (body as { error?: { code?: string; message?: string; requestId?: string } })
      ?.error;
    throw new BridgeError(
      error?.message ?? `Bridge returned HTTP ${response.status}.`,
      error?.code ?? `HTTP_${response.status}`,
      response.status,
      error?.requestId,
    );
  }
  return body as T;
}

export async function fetchHealth(baseUrl: string): Promise<void> {
  const health = await request<{ status: string }>(`${baseUrl}/healthz`, { method: 'GET' });
  if (health.status !== 'ok') {
    throw new BridgeError('Bridge health check did not report ok.', 'UNHEALTHY');
  }
}

export function pair(baseUrl: string, code: string): Promise<PairResponse> {
  return request<PairResponse>(`${baseUrl}/api/v1/pair`, {
    method: 'POST',
    body: JSON.stringify({ code: code.trim().toUpperCase() }),
  });
}

export function fetchStatus(baseUrl: string, token: string): Promise<StatusResponse> {
  return request<StatusResponse>(`${baseUrl}/api/v1/status`, { method: 'GET' }, token);
}

export async function revokeToken(baseUrl: string, token: string): Promise<void> {
  await request<void>(`${baseUrl}/api/v1/tokens/revoke`, { method: 'POST' }, token);
}

export interface SocketHandlers {
  onEvent: (event: BridgeEvent) => void;
  onState: (state: 'connecting' | 'live' | 'paused') => void;
  onUnauthorized: () => void;
  onError: (message: string) => void;
}

/** First-message-auth WebSocket; no token ever appears in the URL. */
export class BridgeSocket {
  private socket: WebSocket | null = null;
  private intentionalClose = false;
  private readonly eventUrl: string;
  private readonly token: string;
  private readonly handlers: SocketHandlers;

  constructor(eventUrl: string, token: string, handlers: SocketHandlers) {
    this.eventUrl = eventUrl;
    this.token = token;
    this.handlers = handlers;
  }

  connect(): void {
    this.intentionalClose = false;
    this.handlers.onState('connecting');
    let socket: WebSocket;
    try {
      socket = new WebSocket(this.eventUrl);
    } catch {
      this.handlers.onError('Cannot open the bridge event stream.');
      this.handlers.onState('paused');
      return;
    }
    this.socket = socket;

    socket.onopen = () => {
      socket.send(JSON.stringify({ type: 'authenticate', token: this.token }));
    };
    socket.onmessage = (message) => {
      if (typeof message.data !== 'string') {
        return;
      }
      let event: BridgeEvent;
      try {
        event = JSON.parse(message.data) as BridgeEvent;
      } catch {
        this.handlers.onError('Bridge sent a malformed event frame.');
        return;
      }
      if (event.type === 'bridge.status') {
        this.handlers.onState('live');
      }
      this.handlers.onEvent(event);
    };
    socket.onerror = () => {
      if (!this.intentionalClose) {
        this.handlers.onError('Bridge event stream failed.');
      }
    };
    socket.onclose = (event) => {
      if (this.socket === socket) {
        this.socket = null;
      }
      if (this.intentionalClose) {
        return;
      }
      if (event.code === 4401) {
        this.handlers.onUnauthorized();
        return;
      }
      this.handlers.onState('paused');
    };
  }

  close(): void {
    this.intentionalClose = true;
    this.socket?.close(1000, 'client closed');
    this.socket = null;
  }
}
