use chrono::{DateTime, SecondsFormat, Utc};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use super::Accuracy;

/// Stable agent identifier, e.g. `"codex"`. Collectors own their constant.
pub type AgentId = &'static str;

/// A project/repository an event belongs to. `root_path` is the canonical identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectRef {
    pub root_path: String,
    pub name: String,
}

/// One normalised usage record.
///
/// Counters are **disjoint** so agents with different "input" semantics stay comparable:
/// `input_tokens` is fresh (non-cached) input; cache reads/writes are separate;
/// `output_tokens` includes reasoning, and `reasoning_tokens` is an informational subset.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageEvent {
    pub agent: AgentId,
    pub model: String,
    /// UTC unix milliseconds.
    pub ts_utc_ms: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// `None` = the agent does not say (distinct from 0).
    pub reasoning_tokens: Option<u64>,
    pub session_id: Option<String>,
    pub project: Option<ProjectRef>,
    /// Where the number came from, e.g. `"local-session-data"`.
    pub source: &'static str,
    pub accuracy: Accuracy,
    /// Globally unique per upstream record; makes ingestion idempotent.
    pub dedupe_key: String,
}

impl UsageEvent {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }

    /// True when every counter is zero (placeholder rows that carry no usage).
    pub fn is_empty(&self) -> bool {
        self.total_tokens() == 0
    }
}

/// Serialises in the shape given in the product spec (camelCase, ISO-8601 UTC timestamp).
impl Serialize for UsageEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let ts = DateTime::<Utc>::from_timestamp_millis(self.ts_utc_ms)
            .map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true))
            .unwrap_or_default();
        let mut st = s.serialize_struct("UsageEvent", 11)?;
        st.serialize_field("agent", self.agent)?;
        st.serialize_field("model", &self.model)?;
        st.serialize_field("timestamp", &ts)?;
        st.serialize_field("inputTokens", &self.input_tokens)?;
        st.serialize_field("outputTokens", &self.output_tokens)?;
        st.serialize_field("cachedTokens", &self.cache_read_tokens)?;
        st.serialize_field("cacheWriteTokens", &self.cache_write_tokens)?;
        st.serialize_field("reasoningTokens", &self.reasoning_tokens)?;
        st.serialize_field("totalTokens", &self.total_tokens())?;
        st.serialize_field("source", self.source)?;
        st.serialize_field("accuracy", &self.accuracy)?;
        st.end()
    }
}

#[cfg(test)]
pub(crate) fn sample_event(agent: AgentId, key: &str, ts_utc_ms: i64) -> UsageEvent {
    UsageEvent {
        agent,
        model: "model-a".into(),
        ts_utc_ms,
        input_tokens: 1000,
        output_tokens: 200,
        cache_read_tokens: 300,
        cache_write_tokens: 0,
        reasoning_tokens: Some(50),
        session_id: Some("s1".into()),
        project: None,
        source: "local-session-data",
        accuracy: Accuracy::Real,
        dedupe_key: key.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_is_sum_of_disjoint_counters() {
        let e = sample_event("codex", "k", 0);
        assert_eq!(e.total_tokens(), 1500);
        assert!(!e.is_empty());
    }

    #[test]
    fn serialises_in_spec_shape() {
        // 2026-10-07T12:30:00Z
        let e = sample_event("codex", "k", 1_791_376_200_000);
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["agent"], "codex");
        assert_eq!(v["timestamp"], "2026-10-07T12:30:00Z");
        assert_eq!(v["inputTokens"], 1000);
        assert_eq!(v["outputTokens"], 200);
        assert_eq!(v["cachedTokens"], 300);
        assert_eq!(v["totalTokens"], 1500);
        assert_eq!(v["source"], "local-session-data");
        assert_eq!(v["accuracy"], "actual");
    }

    #[test]
    fn unknown_reasoning_serialises_as_null_not_zero() {
        let mut e = sample_event("codex", "k", 0);
        e.reasoning_tokens = None;
        let v = serde_json::to_value(&e).unwrap();
        assert!(v["reasoningTokens"].is_null());
    }
}
