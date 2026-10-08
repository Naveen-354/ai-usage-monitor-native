//! Frontend-agnostic facade over the backend: everything a UI needs, with no GUI types.
//!
//! It is the whole surface a UI needs (`overview`, `settings`, `update_settings`, `collector_health`, `rescan`, ...):
//! period boundaries are *local* calendar periods (data is stored in UTC), custom ranges are validated, and a
//! database that was damaged and rebuilt is reported instead of being silent.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use chrono::{Local, NaiveDate, Utc};

use crate::aggregation::{build_overview, custom_range, range_for, Overview, Period};
use crate::collectors::{self, Env};
use crate::database::{queries, Database};
use crate::error::{AppError, Result};
use crate::model::CollectorHealth;
use crate::monitoring::{Notifier, Supervisor};
use crate::settings::{Settings, SettingsStore};

/// What changed in the backend. The UI re-reads the data it shows when it receives one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEvent {
    /// New usage was ingested.
    OverviewChanged,
    /// A collector's health changed (shares the trigger with `OverviewChanged`).
    HealthChanged,
}

pub struct Config {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    /// `false` opens the database without starting the collectors (tests, read-only tools).
    pub monitoring_on: bool,
}

impl Config {
    /// The default layout: `<data_dir>/usage.db`, monitoring on.
    pub fn in_dir(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self { db_path: data_dir.join("usage.db"), data_dir, monitoring_on: true }
    }
}

type Subscribers = Arc<Mutex<Vec<Sender<CoreEvent>>>>;

struct ChannelNotifier(Subscribers);

impl Notifier for ChannelNotifier {
    fn usage_updated(&self) {
        // A subscriber whose receiver was dropped is removed on the next notification.
        if let Ok(mut subs) = self.0.lock() {
            subs.retain(|tx| tx.send(CoreEvent::OverviewChanged).is_ok() && tx.send(CoreEvent::HealthChanged).is_ok());
        }
    }
}

pub struct Core {
    db: Arc<Database>,
    settings: Arc<SettingsStore>,
    supervisor: Option<Supervisor>,
    subscribers: Subscribers,
    data_dir: PathBuf,
    db_notice: Option<String>,
}

fn parse_date(label: &str, v: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| AppError::Invalid(format!("{label} must be YYYY-MM-DD, got '{v}'")))
}

impl Core {
    pub fn start(config: Config) -> Result<Self> {
        std::fs::create_dir_all(&config.data_dir)?;
        let (db, report) = Database::open_with_report(&config.db_path)?;
        let settings = Arc::new(SettingsStore::load(db.clone())?);
        let subscribers: Subscribers = Arc::new(Mutex::new(Vec::new()));

        let supervisor = config.monitoring_on.then(|| {
            Supervisor::start(
                db.clone(),
                settings.clone(),
                Env::from_system(),
                collectors::registry(),
                Arc::new(ChannelNotifier(subscribers.clone())),
            )
        });

        Ok(Core { db, settings, supervisor, subscribers, data_dir: config.data_dir, db_notice: report.notice() })
    }

    /// Usage for a local calendar period. `from`/`to` (`YYYY-MM-DD`, inclusive) are required for [`Period::Custom`].
    pub fn overview(&self, period: Period, from: Option<&str>, to: Option<&str>) -> Result<Overview> {
        let settings = self.settings.get();
        let now = Utc::now();
        let range = if period == Period::Custom {
            let (Some(from), Some(to)) = (from, to) else {
                return Err(AppError::Invalid("a custom period needs both from and to".into()));
            };
            custom_range(&Local, parse_date("from", from)?, parse_date("to", to)?)
                .ok_or_else(|| AppError::Invalid("range end is before its start".into()))?
        } else {
            range_for(&Local, now, period, settings.week_starts_on)
        };
        let mut overview = self.db.with_reader(|c| build_overview(c, &settings, period, range, now.timestamp_millis()))?;
        if let Some(sup) = &self.supervisor {
            overview.importing_agents = sup.importing_agents();
        }
        Ok(overview)
    }

    pub fn settings(&self) -> Settings {
        self.settings.get()
    }

    /// Applies a partial settings document (camelCase keys), validates/clamps it, persists it, and wakes the monitor.
    pub fn update_settings(&self, patch: &serde_json::Value) -> Result<Settings> {
        let updated = self.settings.update(patch)?;
        if let Some(sup) = &self.supervisor {
            sup.refresh_now();
        }
        Ok(updated)
    }

    pub fn collector_health(&self) -> Result<Vec<CollectorHealth>> {
        self.db.with_reader(queries::load_health)
    }

    /// Plain-language note when the database was damaged and rebuilt at start-up.
    pub fn database_notice(&self) -> Option<&str> {
        self.db_notice.as_deref()
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn database_path(&self) -> &Path {
        self.db.path()
    }

    /// Asks the collectors to run now instead of waiting for the next poll.
    pub fn rescan(&self) {
        if let Some(sup) = &self.supervisor {
            sup.refresh_now();
        }
    }

    pub fn subscribe(&self) -> Receiver<CoreEvent> {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.push(tx);
        }
        rx
    }

    /// Stops the collectors and folds the write-ahead log back into the database file.
    pub fn shutdown(self) {
        if let Some(sup) = self.supervisor {
            sup.shutdown();
        }
        let _ = self.db.checkpoint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregation::local_midnight;

    fn core_in(dir: &tempfile::TempDir) -> Core {
        let mut cfg = Config::in_dir(dir.path());
        cfg.monitoring_on = false;
        Core::start(cfg).unwrap()
    }

    #[test]
    fn an_empty_database_gives_an_overview_with_no_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let core = core_in(&dir);
        let o = core.overview(Period::Day, None, None).unwrap();
        assert!(!o.any_data);
        assert!(core.database_notice().is_none());
        core.shutdown();
    }

    #[test]
    fn day_starts_at_local_midnight_not_utc_midnight() {
        let dir = tempfile::tempdir().unwrap();
        let core = core_in(&dir);
        let o = core.overview(Period::Day, None, None).unwrap();
        let expected = local_midnight(&Local, Local::now().date_naive()).timestamp_millis();
        assert_eq!(o.range.start_utc_ms, expected);
        core.shutdown();
    }

    #[test]
    fn custom_ranges_are_validated_like_the_old_commands() {
        let dir = tempfile::tempdir().unwrap();
        let core = core_in(&dir);
        assert!(core.overview(Period::Custom, None, None).is_err());
        assert!(core.overview(Period::Custom, Some("2026-10-01"), None).is_err());
        assert!(core.overview(Period::Custom, Some("nope"), Some("2026-10-02")).is_err());
        assert!(core.overview(Period::Custom, Some("2026-10-05"), Some("2026-10-01")).is_err());
        let ok = core.overview(Period::Custom, Some("2026-10-01"), Some("2026-10-02")).unwrap();
        assert_eq!(ok.range.end_utc_ms - ok.range.start_utc_ms, 2 * 24 * 3_600_000);
        core.shutdown();
    }

    #[test]
    fn settings_updates_are_clamped_and_persist_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let core = core_in(&dir);
        let s = core.update_settings(&serde_json::json!({"tokenTextSize": 99.0, "alwaysOnTop": false})).unwrap();
        assert!(s.token_text_size <= 2.0);
        assert!(!s.always_on_top);
        core.shutdown();
        let again = core_in(&dir);
        assert!(!again.settings().always_on_top);
        again.shutdown();
    }

    #[test]
    fn a_damaged_database_is_set_aside_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("usage.db"), vec![0xAB_u8; 8192]).unwrap();
        let core = core_in(&dir);
        assert!(core.database_notice().is_some(), "the user must be told the database was rebuilt");
        assert!(core.overview(Period::Day, None, None).is_ok());
        core.shutdown();
    }

    #[test]
    fn collector_health_is_readable_and_subscribers_can_attach() {
        let dir = tempfile::tempdir().unwrap();
        let core = core_in(&dir);
        assert!(core.collector_health().unwrap().is_empty());
        let _rx = core.subscribe();
        core.rescan();
        core.shutdown();
    }
}
