use serde::{Deserialize, Serialize};

/// What the monitor can honestly say about an agent right now.
///
/// Only `Ok` and `NoDataYet` may be rendered as numbers (a `NoDataYet` zero is a real zero).
/// Everything else must render as unavailable — never as `0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Availability {
    /// Counts were read from verified local data.
    Ok,
    /// Agent present and readable, but no usage recorded yet.
    NoDataYet,
    /// Agent not installed and no data found.
    NotInstalled,
    /// Agent exists but exposes no reliable local token data (or the format is unrecognised).
    Unavailable { reason: String },
    /// The collector itself failed (isolated; other collectors are unaffected).
    Error { message: String },
}

impl Availability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Availability::Ok => "ok",
            Availability::NoDataYet => "no-data-yet",
            Availability::NotInstalled => "not-installed",
            Availability::Unavailable { .. } => "unavailable",
            Availability::Error { .. } => "error",
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Availability::Unavailable { reason } => Some(reason),
            Availability::Error { message } => Some(message),
            _ => None,
        }
    }

    pub fn from_parts(state: &str, detail: Option<String>) -> Availability {
        match state {
            "ok" => Availability::Ok,
            "no-data-yet" => Availability::NoDataYet,
            "unavailable" => Availability::Unavailable { reason: detail.unwrap_or_default() },
            "error" => Availability::Error { message: detail.unwrap_or_default() },
            _ => Availability::NotInstalled,
        }
    }

    /// May numbers be shown for this state?
    pub fn has_numbers(&self) -> bool {
        matches!(self, Availability::Ok | Availability::NoDataYet)
    }
}

/// Health snapshot exposed in Settings → Diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorHealth {
    pub agent: String,
    pub availability: Availability,
    /// Human-readable note, e.g. "data found, CLI not on PATH; last activity 2026-07-12".
    pub note: Option<String>,
    pub last_run_utc_ms: Option<i64>,
    pub last_success_utc_ms: Option<i64>,
    pub last_event_utc_ms: Option<i64>,
    pub events_total: u64,
    /// Records seen but skipped (zero usage, undatable, unparsable...). Never silently dropped.
    pub skipped_records: u64,
    pub source_paths: Vec<String>,
}

impl CollectorHealth {
    pub fn new(agent: &str, availability: Availability) -> Self {
        CollectorHealth {
            agent: agent.to_string(),
            availability,
            note: None,
            last_run_utc_ms: None,
            last_success_utc_ms: None,
            last_event_utc_ms: None,
            events_total: 0,
            skipped_records: 0,
            source_paths: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ok_and_no_data_show_numbers() {
        assert!(Availability::Ok.has_numbers());
        assert!(Availability::NoDataYet.has_numbers());
        assert!(!Availability::NotInstalled.has_numbers());
        assert!(!Availability::Unavailable { reason: "x".into() }.has_numbers());
        assert!(!Availability::Error { message: "x".into() }.has_numbers());
    }

    #[test]
    fn round_trips_through_db_representation() {
        for a in [
            Availability::Ok,
            Availability::NoDataYet,
            Availability::NotInstalled,
            Availability::Unavailable { reason: "r".into() },
            Availability::Error { message: "m".into() },
        ] {
            let back = Availability::from_parts(a.as_str(), a.detail().map(str::to_string));
            assert_eq!(a, back);
        }
    }

    #[test]
    fn json_is_tagged_by_state() {
        let v = serde_json::to_value(Availability::Unavailable { reason: "no ledger".into() }).unwrap();
        assert_eq!(v["state"], "unavailable");
        assert_eq!(v["reason"], "no ledger");
    }
}
