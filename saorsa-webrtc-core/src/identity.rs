//! Peer identity abstraction
//!
//! This module provides traits and types for peer identity in the WebRTC system.
//! It allows the library to work with any identity system, including FourWordAddress
//! from saorsa-core or custom identity implementations.

use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::hash::Hash;

/// Trait for peer identity in WebRTC system
///
/// Implementations must provide a way to uniquely identify peers in the network.
/// The identity must be serializable, comparable, and displayable.
pub trait PeerIdentity:
    Clone + Debug + Display + Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static
{
    /// Convert the identity to a string representation
    fn to_string_repr(&self) -> String;

    /// Try to create an identity from a string representation
    fn from_string_repr(s: &str) -> anyhow::Result<Self>
    where
        Self: Sized;

    /// Get a unique identifier for this peer (for use in hash maps, etc.)
    fn unique_id(&self) -> String {
        self.to_string_repr()
    }
}

/// Simple string-based peer identity
///
/// This is a basic implementation that uses strings as peer identifiers.
/// Suitable for testing or simple applications. For production use, consider
/// using more robust identity systems like FourWordAddress from saorsa-core.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerIdentityString(pub String);

impl PeerIdentityString {
    /// Create a new string-based peer identity
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Get the inner string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for PeerIdentityString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PeerIdentity for PeerIdentityString {
    fn to_string_repr(&self) -> String {
        self.0.clone()
    }

    fn from_string_repr(s: &str) -> anyhow::Result<Self> {
        Ok(Self(s.to_string()))
    }
}

impl From<&str> for PeerIdentityString {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for PeerIdentityString {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_identity_string() {
        let id = PeerIdentityString::new("alice-bob-charlie-david");
        assert_eq!(id.to_string(), "alice-bob-charlie-david");
        assert_eq!(id.to_string_repr(), "alice-bob-charlie-david");
    }

    #[test]
    fn test_peer_identity_from_string() {
        let id = PeerIdentityString::from_string_repr("test-peer-id")
            .ok()
            .unwrap();
        assert_eq!(id.as_str(), "test-peer-id");
    }

    #[test]
    fn test_peer_identity_serialization() {
        let id = PeerIdentityString::new("alice-bob");
        let json = serde_json::to_string(&id).ok().unwrap();
        let deserialized: PeerIdentityString = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(id, deserialized);
    }
}
