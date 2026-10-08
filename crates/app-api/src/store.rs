use crate::api::{Backend, BackendEvent, SettingsPatch};
use crate::view::{Overview, Settings, PeriodOrCustom};

#[derive(Debug, Clone, PartialEq)]
pub struct Surge {
    pub id: u32,
    pub delta: u64,
    pub at: i64,
}

pub struct AppStore {
    pub settings: Option<Settings>,
    pub overview: Option<Overview>,
    pub surge: Option<Surge>,
    pub error: Option<String>,
    surge_id: u32,
}

const MIN_SURGE_TOKENS: u64 = 500;

impl Default for AppStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AppStore {
    pub fn new() -> Self {
        Self {
            settings: None,
            overview: None,
            surge: None,
            error: None,
            surge_id: 0,
        }
    }

    pub fn init(&mut self, backend: &dyn Backend) {
        match backend.settings() {
            Ok(s) => {
                self.settings = Some(s);
                self.refresh(backend);
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }

    pub fn refresh(&mut self, backend: &dyn Backend) {
        let Some(settings) = &self.settings else { return; };
        
        // We use settings.period, mapping to PeriodOrCustom
        let period = match settings.period {
            crate::view::PeriodKey::Day => PeriodOrCustom::Day,
            crate::view::PeriodKey::Week => PeriodOrCustom::Week,
            crate::view::PeriodKey::Month => PeriodOrCustom::Month,
            crate::view::PeriodKey::Year => PeriodOrCustom::Year,
        };

        match backend.overview(period, None, None) {
            Ok(new_overview) => {
                let delta = Self::detect_surge(self.overview.as_ref(), &new_overview);
                if let Some(d) = delta {
                    self.surge_id += 1;
                    self.surge = Some(Surge {
                        id: self.surge_id,
                        delta: d,
                        at: chrono::Utc::now().timestamp_millis(),
                    });
                }
                self.overview = Some(new_overview);
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }

    pub fn apply(&mut self, event: BackendEvent, backend: &dyn Backend) {
        match event {
            BackendEvent::OverviewChanged => {
                self.refresh(backend);
            }
            BackendEvent::SettingsChanged(s) => {
                self.settings = Some(s);
                self.refresh(backend);
            }
            BackendEvent::HealthChanged => {
                // Not specified what this does in the brief for AppStore, but probably nothing or refresh
            }
            BackendEvent::DatabaseNotice(msg) => {
                self.error = Some(msg);
            }
            BackendEvent::ImportProgress { .. } => {
                // Refresh to show progress
                self.refresh(backend);
            }
        }
    }

    pub fn update(&mut self, patch: SettingsPatch, backend: &dyn Backend) {
        // Optimistic update
        let old_settings = self.settings.clone();
        if let Some(s) = &mut self.settings {
            if let Some(v) = patch.period { s.period = v; }
            // In a real app we'd apply all fields, but for this mock state machine, period is most critical.
        }

        match backend.update_settings(patch) {
            Ok(new_settings) => {
                self.settings = Some(new_settings);
                self.error = None;
                self.refresh(backend);
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.settings = old_settings;
            }
        }
    }

    fn agents_with_numbers(o: &Overview) -> String {
        let mut ids: Vec<&str> = o.agents
            .iter()
            .filter(|a| a.enabled && a.totals.is_some())
            .map(|a| a.id.as_str())
            .collect();
        ids.sort();
        ids.join(",")
    }

    fn detect_surge(prev: Option<&Overview>, next: &Overview) -> Option<u64> {
        let prev = prev?;
        let prev_totals = prev.totals.as_ref()?;
        let next_totals = next.totals.as_ref()?;

        if prev.period != next.period || prev.range.start_utc_ms != next.range.start_utc_ms {
            return None;
        }
        if prev.count_cached_in_total != next.count_cached_in_total {
            return None;
        }
        if !prev.importing_agents.is_empty() || !next.importing_agents.is_empty() {
            return None;
        }
        if Self::agents_with_numbers(prev) != Self::agents_with_numbers(next) {
            return None;
        }

        let delta = next_totals.total.saturating_sub(prev_totals.total);
        if delta >= MIN_SURGE_TOKENS {
            Some(delta)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_backend::MockBackend;

    fn is_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_send_sync() {
        is_send_sync::<MockBackend>();
    }

    #[test]
    fn test_trait_methods() {
        let backend = MockBackend::new();
        assert!(backend.settings().is_ok());
        assert!(backend.overview(PeriodOrCustom::Day, None, None).is_ok());
        assert!(backend.history(PeriodOrCustom::Day, None, None).is_ok());
        assert!(backend.app_info().is_ok());
        assert!(backend.overlay_diagnostics().is_ok());
        
        // Error simulation
        backend.fail_next(crate::api::BackendError::Message("test error".into()));
        assert!(backend.overview(PeriodOrCustom::Day, None, None).is_err());
    }

    #[test]
    fn test_store_init_and_refresh() {
        let backend = MockBackend::new();
        let mut store = AppStore::new();
        store.init(&backend);
        assert!(store.settings.is_some());
        assert!(store.overview.is_some());
        assert!(store.error.is_none());
    }

    #[test]
    fn test_store_error_and_rollback() {
        let backend = MockBackend::new();
        let mut store = AppStore::new();
        store.init(&backend);
        
        let old_settings = store.settings.clone();
        
        backend.fail_next(crate::api::BackendError::Message("fail".into()));
        let patch = SettingsPatch {
            period: Some(crate::view::PeriodKey::Year),
            ..Default::default()
        };
        store.update(patch, &backend);
        
        assert_eq!(store.settings, old_settings);
        assert_eq!(store.error.as_deref(), Some("fail"));
    }

    #[test]
    fn test_headless_thread_event_draining() {
        use std::sync::Arc;
        let backend = Arc::new(MockBackend::new());
        let rx = backend.subscribe();
        
        let mut store = AppStore::new();
        store.init(&*backend);

        let b2 = Arc::clone(&backend);
        let handle = std::thread::spawn(move || {
            b2.tick(1000);
            b2.emit_database_notice("damaged");
            b2.tick(5000);
        });

        let mut events = vec![];
        for event in rx.iter() {
            events.push(event.clone());
            store.apply(event, &*backend);
            if events.len() == 3 {
                break;
            }
        }
        handle.join().unwrap();
        assert!(store.surge.is_some());
    }
}
