// SPDX-License-Identifier: GPL-3.0-or-later
//! Credentials. OAuth2/SSO is not implemented; it plugs in as another
//! [`AuthProvider`].

use std::fmt;

use reqwest::header::{HeaderMap, HeaderValue};

use super::NexusError;

/// A Nexus personal API key. `Debug` is redacted and there is no `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Trims surrounding whitespace (legacy strips it on login) and rejects
    /// empty keys or keys that cannot be sent as a header value.
    pub fn new(raw: &str) -> Result<Self, NexusError> {
        let key = raw.trim();
        let ok = !key.is_empty() && key.len() <= 4096 && key.bytes().all(|b| b.is_ascii_graphic());
        if ok {
            Ok(Self(key.to_owned()))
        } else {
            Err(NexusError::InvalidKey)
        }
    }

    /// The raw key. Only for the header and the secret store.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(***)")
    }
}

/// Download key from an nxm:// link (free accounts). `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct NxmKey {
    pub key: String,
    /// Unix time after which Nexus refuses the key.
    pub expires: i64,
    pub user_id: Option<u64>,
}

impl NxmKey {
    pub fn is_expired(&self, now: i64) -> bool {
        self.expires <= now
    }
}

impl fmt::Debug for NxmKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NxmKey")
            .field("key", &"***")
            .field("expires", &self.expires)
            .field("user_id", &self.user_id)
            .finish()
    }
}

/// Adds credentials to every Nexus request.
pub trait AuthProvider: Send + Sync + fmt::Debug {
    /// Inserts the auth headers. Header values must be marked sensitive.
    fn apply(&self, headers: &mut HeaderMap) -> Result<(), NexusError>;

    /// Replaces any secret this provider holds in `text` (error bodies are
    /// echoed into messages).
    fn redact(&self, text: &str) -> String {
        text.to_owned()
    }
}

/// Personal API key sent as the `apikey` header.
#[derive(Debug, Clone)]
pub struct ApiKeyAuth {
    key: ApiKey,
}

impl ApiKeyAuth {
    pub fn new(key: ApiKey) -> Self {
        Self { key }
    }
}

impl AuthProvider for ApiKeyAuth {
    fn apply(&self, headers: &mut HeaderMap) -> Result<(), NexusError> {
        let mut v = HeaderValue::from_str(self.key.expose()).map_err(|_| NexusError::InvalidKey)?;
        v.set_sensitive(true);
        headers.insert("apikey", v);
        Ok(())
    }

    fn redact(&self, text: &str) -> String {
        text.replace(self.key.expose(), "***")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_trimmed_and_debug_redacted() {
        let k = ApiKey::new("  abc123==\n").unwrap();
        assert_eq!(k.expose(), "abc123==");
        assert_eq!(format!("{k:?}"), "ApiKey(***)");
    }

    #[test]
    fn empty_or_control_keys_rejected() {
        assert!(matches!(ApiKey::new("  "), Err(NexusError::InvalidKey)));
        assert!(matches!(ApiKey::new("a b"), Err(NexusError::InvalidKey)));
    }

    #[test]
    fn nxm_key_debug_redacted() {
        let k = NxmKey {
            key: "SECRETKEY".into(),
            expires: 5,
            user_id: Some(1),
        };
        assert!(!format!("{k:?}").contains("SECRETKEY"));
        assert!(k.is_expired(5));
        assert!(!k.is_expired(4));
    }
}
