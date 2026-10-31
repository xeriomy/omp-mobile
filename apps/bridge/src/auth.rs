//! Short-lived single-use pairing codes and in-memory client tokens.
//!
//! Only SHA-256 hashes of pairing codes and tokens are retained. Tokens are
//! intentionally not persisted: a bridge restart requires pairing again.

use getrandom::fill;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 10;
const MAX_PAIRING_ATTEMPTS: u8 = 5;
const DEFAULT_TOKEN_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedToken {
    pub token: String,
    pub token_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairError {
    Invalid,
    Expired,
    Locked,
}

struct Pairing {
    code_hash: [u8; 32],
    expires_at: Instant,
    attempts: u8,
}

struct TokenEntry {
    token_id: String,
    expires_at: Instant,
}

#[derive(Default)]
struct Inner {
    pairing: Mutex<Option<Pairing>>,
    tokens: RwLock<HashMap<[u8; 32], TokenEntry>>,
}

#[derive(Clone)]
pub struct AuthStore {
    inner: std::sync::Arc<Inner>,
    token_ttl: Duration,
}

impl Default for AuthStore {
    fn default() -> Self {
        Self {
            inner: std::sync::Arc::default(),
            token_ttl: DEFAULT_TOKEN_TTL,
        }
    }
}

impl AuthStore {
    pub fn new(code: impl AsRef<str>, ttl: Duration) -> Self {
        Self::new_with_token_ttl(code, ttl, DEFAULT_TOKEN_TTL)
    }

    pub fn new_with_token_ttl(
        code: impl AsRef<str>,
        pairing_ttl: Duration,
        token_ttl: Duration,
    ) -> Self {
        let store = Self {
            inner: std::sync::Arc::default(),
            token_ttl,
        };
        store.replace_pairing_code(code.as_ref(), pairing_ttl);
        store
    }

    pub fn with_generated_code(pairing_ttl: Duration, token_ttl: Duration) -> (Self, String) {
        let code = generate_code();
        (
            Self::new_with_token_ttl(&code, pairing_ttl, token_ttl),
            code,
        )
    }

    /// Replace the active pairing code. Existing tokens stay valid.
    pub fn replace_pairing_code(&self, code: &str, ttl: Duration) {
        let pairing = Pairing {
            code_hash: hash(code.as_bytes()),
            expires_at: Instant::now() + ttl,
            attempts: 0,
        };
        *self.inner.pairing.lock().expect("pairing lock") = Some(pairing);
    }

    pub fn pair(&self, code: &str) -> Result<IssuedToken, PairError> {
        let mut guard = self.inner.pairing.lock().expect("pairing lock");
        let Some(pairing) = guard.as_mut() else {
            return Err(PairError::Invalid);
        };
        if Instant::now() >= pairing.expires_at {
            *guard = None;
            return Err(PairError::Expired);
        }
        if hash(code.as_bytes()) != pairing.code_hash {
            pairing.attempts = pairing.attempts.saturating_add(1);
            if pairing.attempts >= MAX_PAIRING_ATTEMPTS {
                *guard = None;
                return Err(PairError::Locked);
            }
            return Err(PairError::Invalid);
        }

        // A successful exchange consumes the code before issuing a token.
        *guard = None;
        drop(guard);

        let token = random_token();
        let token_id = random_id();
        let expires_at = Instant::now() + self.token_ttl;
        self.inner.tokens.write().expect("token lock").insert(
            hash(token.as_bytes()),
            TokenEntry {
                token_id: token_id.clone(),
                expires_at,
            },
        );
        Ok(IssuedToken {
            token,
            token_id,
            expires_at: unix_millis_after(self.token_ttl),
        })
    }

    pub fn authorize(&self, token: &str) -> Option<String> {
        if token.is_empty() {
            return None;
        }
        let key = hash(token.as_bytes());
        let mut tokens = self.inner.tokens.write().expect("token lock");
        match tokens.get(&key) {
            Some(entry) if Instant::now() < entry.expires_at => Some(entry.token_id.clone()),
            Some(_) => {
                tokens.remove(&key);
                None
            }
            None => None,
        }
    }

    pub fn revoke(&self, token: &str) -> bool {
        self.inner
            .tokens
            .write()
            .expect("token lock")
            .remove(&hash(token.as_bytes()))
            .is_some()
    }

    #[cfg(test)]
    fn token_count(&self) -> usize {
        self.inner.tokens.read().expect("token lock").len()
    }
}

pub fn generate_code() -> String {
    random_string(CODE_LEN, CODE_ALPHABET)
}

fn random_token() -> String {
    use base64::Engine;
    let mut bytes = [0_u8; 32];
    fill(&mut bytes).expect("OS random source");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random_id() -> String {
    let mut bytes = [0_u8; 8];
    fill(&mut bytes).expect("OS random source");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn random_string(len: usize, alphabet: &[u8]) -> String {
    let mut bytes = vec![0_u8; len];
    fill(&mut bytes).expect("OS random source");
    bytes
        .into_iter()
        .map(|byte| char::from(alphabet[usize::from(byte) % alphabet.len()]))
        .collect()
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn unix_millis_after(ttl: Duration) -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from((elapsed + ttl).as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

pub fn new_request_id() -> String {
    let mut bytes = [0_u8; 8];
    fill(&mut bytes).expect("OS random source");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_code_is_single_use() {
        let store = AuthStore::new("ABCDEFGHJK", Duration::from_secs(60));
        let issued = store.pair("ABCDEFGHJK").unwrap();
        assert_eq!(
            store.authorize(&issued.token).as_deref(),
            Some(issued.token_id.as_str())
        );
        assert_eq!(store.pair("ABCDEFGHJK"), Err(PairError::Invalid));
    }

    #[test]
    fn expired_pairing_code_is_rejected() {
        let store = AuthStore::new("ABCDEFGHJK", Duration::ZERO);
        assert_eq!(store.pair("ABCDEFGHJK"), Err(PairError::Expired));
    }

    #[test]
    fn repeated_wrong_codes_lock_the_pairing_window() {
        let store = AuthStore::new("ABCDEFGHJK", Duration::from_secs(60));
        assert_eq!(store.pair("ZZZZZZZZZZ"), Err(PairError::Invalid));
        assert_eq!(store.pair("ZZZZZZZZZZ"), Err(PairError::Invalid));
        assert_eq!(store.pair("ZZZZZZZZZZ"), Err(PairError::Invalid));
        assert_eq!(store.pair("ZZZZZZZZZZ"), Err(PairError::Invalid));
        assert_eq!(store.pair("ZZZZZZZZZZ"), Err(PairError::Locked));
        assert_eq!(store.pair("ABCDEFGHJK"), Err(PairError::Invalid));
    }

    #[test]
    fn expired_tokens_are_rejected_and_removed() {
        let store =
            AuthStore::new_with_token_ttl("ABCDEFGHJK", Duration::from_secs(60), Duration::ZERO);
        let issued = store.pair("ABCDEFGHJK").unwrap();
        assert!(issued.expires_at > 0);
        assert!(store.authorize(&issued.token).is_none());
        assert_eq!(store.token_count(), 0);
    }

    #[test]
    fn tokens_can_be_revoked() {
        let store = AuthStore::new("ABCDEFGHJK", Duration::from_secs(60));
        let issued = store.pair("ABCDEFGHJK").unwrap();
        assert!(store.authorize(&issued.token).is_some());
        assert!(store.revoke(&issued.token));
        assert!(store.authorize(&issued.token).is_none());
        assert!(!store.revoke(&issued.token));
    }

    #[test]
    fn unknown_tokens_are_rejected() {
        let store = AuthStore::new("ABCDEFGHJK", Duration::from_secs(60));
        assert!(store.authorize("not-a-token").is_none());
        assert!(store.authorize("").is_none());
        assert_eq!(store.token_count(), 0);
    }

    #[test]
    fn generated_codes_use_the_unambiguous_alphabet() {
        let code = generate_code();
        assert_eq!(code.len(), CODE_LEN);
        assert!(code.bytes().all(|byte| CODE_ALPHABET.contains(&byte)));
        assert!(!code.chars().any(|ch| "01IO".contains(ch)));
    }
}
