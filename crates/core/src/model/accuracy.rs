use serde::{Deserialize, Serialize};

/// How trustworthy a token count is.
///
/// * `Real`        – reported by the agent/provider itself ("actual" in JSON).
/// * `Estimated`   – derived or rounded; always labelled in the UI.
/// * `Unavailable` – the agent exposes no reliable local counts. This is a *state*, never a
///   stored value: zero and unknown are different things, so `Unavailable` cannot be persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accuracy {
    #[serde(rename = "actual")]
    Real,
    Estimated,
    Unavailable,
}

impl Accuracy {
    /// Value stored in `usage_events.accuracy`. `None` for `Unavailable`, which must never be stored.
    pub fn storable(self) -> Option<&'static str> {
        match self {
            Accuracy::Real => Some("actual"),
            Accuracy::Estimated => Some("estimated"),
            Accuracy::Unavailable => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_like_the_spec() {
        assert_eq!(serde_json::to_string(&Accuracy::Real).unwrap(), "\"actual\"");
        assert_eq!(serde_json::to_string(&Accuracy::Estimated).unwrap(), "\"estimated\"");
        assert_eq!(serde_json::to_string(&Accuracy::Unavailable).unwrap(), "\"unavailable\"");
    }

    #[test]
    fn unavailable_is_never_storable() {
        assert!(Accuracy::Unavailable.storable().is_none());
        assert_eq!(Accuracy::Real.storable(), Some("actual"));
    }
}
