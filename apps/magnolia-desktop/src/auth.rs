use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use magnolia_domain::ClientId;
use magnolia_protocol::SessionCredential;
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use thiserror::Error;

const AUTHORITY_BYTES: usize = 32;

#[derive(Clone)]
pub struct SessionAuthority {
    inner: Arc<Mutex<AuthorityState>>,
    launch_token: String,
    session_ttl: Duration,
}

struct AuthorityState {
    launch_secret: [u8; AUTHORITY_BYTES],
    launch_expires_at: Instant,
    launch_consumed: bool,
    sessions: BTreeMap<String, SessionRecord>,
}

#[derive(Clone, Copy)]
struct SessionRecord {
    client_id: ClientId,
    expires_at: Instant,
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthenticatedSession {
    pub session_id: String,
    pub resumed: bool,
}

impl fmt::Debug for AuthenticatedSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedSession")
            .field("session_id", &"[redacted]")
            .field("resumed", &self.resumed)
            .finish()
    }
}

impl SessionAuthority {
    pub fn new(launch_ttl: Duration, session_ttl: Duration) -> Result<Self, AuthError> {
        let launch_secret = random_authority()?;
        let launch_token = URL_SAFE_NO_PAD.encode(launch_secret);
        Ok(Self {
            inner: Arc::new(Mutex::new(AuthorityState {
                launch_secret,
                launch_expires_at: Instant::now() + launch_ttl,
                launch_consumed: false,
                sessions: BTreeMap::new(),
            })),
            launch_token,
            session_ttl,
        })
    }

    #[must_use]
    pub fn launch_token(&self) -> &str {
        &self.launch_token
    }

    pub fn authenticate(
        &self,
        credential: &SessionCredential,
        client_id: ClientId,
    ) -> Result<AuthenticatedSession, AuthError> {
        match credential {
            SessionCredential::LaunchToken(candidate) => {
                self.exchange_launch_token(candidate, client_id)
            }
            SessionCredential::SessionId(session_id) => self.resume(session_id, client_id),
        }
    }

    pub fn authenticate_telemetry(&self, session_id: &str) -> Result<ClientId, AuthError> {
        let now = Instant::now();
        let mut state = self.lock()?;
        let record = state
            .sessions
            .get_mut(session_id)
            .ok_or(AuthError::InvalidCredential)?;
        if now >= record.expires_at {
            state.sessions.remove(session_id);
            return Err(AuthError::ExpiredCredential);
        }
        record.expires_at = now + self.session_ttl;
        Ok(record.client_id)
    }

    fn exchange_launch_token(
        &self,
        candidate: &str,
        client_id: ClientId,
    ) -> Result<AuthenticatedSession, AuthError> {
        let decoded = URL_SAFE_NO_PAD
            .decode(candidate)
            .map_err(|_| AuthError::MalformedCredential)?;
        if decoded.len() != AUTHORITY_BYTES {
            return Err(AuthError::MalformedCredential);
        }
        let now = Instant::now();
        let mut state = self.lock()?;
        if now >= state.launch_expires_at {
            return Err(AuthError::ExpiredCredential);
        }
        if state.launch_consumed {
            return Err(AuthError::ConsumedCredential);
        }
        if state.launch_secret.ct_eq(decoded.as_slice()).unwrap_u8() != 1 {
            return Err(AuthError::InvalidCredential);
        }
        state.launch_consumed = true;
        let session_secret = random_authority()?;
        let session_id = URL_SAFE_NO_PAD.encode(session_secret);
        state.sessions.insert(
            session_id.clone(),
            SessionRecord {
                client_id,
                expires_at: now + self.session_ttl,
            },
        );
        Ok(AuthenticatedSession {
            session_id,
            resumed: false,
        })
    }

    fn resume(
        &self,
        session_id: &str,
        client_id: ClientId,
    ) -> Result<AuthenticatedSession, AuthError> {
        if URL_SAFE_NO_PAD
            .decode(session_id)
            .map_err(|_| AuthError::MalformedCredential)?
            .len()
            != AUTHORITY_BYTES
        {
            return Err(AuthError::MalformedCredential);
        }
        let now = Instant::now();
        let mut state = self.lock()?;
        let record = state
            .sessions
            .get_mut(session_id)
            .ok_or(AuthError::InvalidCredential)?;
        if now >= record.expires_at {
            state.sessions.remove(session_id);
            return Err(AuthError::ExpiredCredential);
        }
        if record.client_id != client_id {
            return Err(AuthError::ClientMismatch);
        }
        record.expires_at = now + self.session_ttl;
        Ok(AuthenticatedSession {
            session_id: session_id.to_owned(),
            resumed: true,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, AuthorityState>, AuthError> {
        self.inner.lock().map_err(|_| AuthError::Poisoned)
    }
}

fn random_authority() -> Result<[u8; AUTHORITY_BYTES], AuthError> {
    let mut bytes = [0_u8; AUTHORITY_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| AuthError::Entropy(error.to_string()))?;
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuthError {
    #[error("credential is malformed")]
    MalformedCredential,
    #[error("credential is incorrect")]
    InvalidCredential,
    #[error("credential has expired")]
    ExpiredCredential,
    #[error("launch credential was already consumed")]
    ConsumedCredential,
    #[error("session belongs to a different client")]
    ClientMismatch,
    #[error("session authority lock was poisoned")]
    Poisoned,
    #[error("operating system entropy is unavailable: {0}")]
    Entropy(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_token_is_single_use_but_session_resumes() {
        let authority =
            SessionAuthority::new(Duration::from_secs(30), Duration::from_secs(30)).unwrap();
        let client_id = ClientId::from_u128(1);
        let launch = SessionCredential::LaunchToken(authority.launch_token().to_owned());
        let first = authority.authenticate(&launch, client_id).unwrap();
        assert!(!first.resumed);
        let debug = format!("{first:?}");
        assert!(!debug.contains(&first.session_id));
        assert!(debug.contains("redacted"));
        assert_eq!(
            authority.authenticate(&launch, client_id),
            Err(AuthError::ConsumedCredential)
        );
        let resumed = authority
            .authenticate(
                &SessionCredential::SessionId(first.session_id.clone()),
                client_id,
            )
            .unwrap();
        assert!(resumed.resumed);
        assert_eq!(resumed.session_id, first.session_id);
    }

    #[test]
    fn malformed_wrong_expired_and_cross_client_credentials_are_rejected() {
        let authority =
            SessionAuthority::new(Duration::from_secs(30), Duration::from_secs(30)).unwrap();
        assert_eq!(
            authority.authenticate(
                &SessionCredential::LaunchToken("not-base64!".to_owned()),
                ClientId::from_u128(1),
            ),
            Err(AuthError::MalformedCredential)
        );
        let wrong = URL_SAFE_NO_PAD.encode([7_u8; AUTHORITY_BYTES]);
        assert_eq!(
            authority.authenticate(
                &SessionCredential::LaunchToken(wrong),
                ClientId::from_u128(1),
            ),
            Err(AuthError::InvalidCredential)
        );

        let first = authority
            .authenticate(
                &SessionCredential::LaunchToken(authority.launch_token().to_owned()),
                ClientId::from_u128(1),
            )
            .unwrap();
        assert_eq!(
            authority.authenticate(
                &SessionCredential::SessionId(first.session_id),
                ClientId::from_u128(2),
            ),
            Err(AuthError::ClientMismatch)
        );

        let expired = SessionAuthority::new(Duration::ZERO, Duration::from_secs(30)).unwrap();
        assert_eq!(
            expired.authenticate(
                &SessionCredential::LaunchToken(expired.launch_token().to_owned()),
                ClientId::from_u128(1),
            ),
            Err(AuthError::ExpiredCredential)
        );
    }
}
