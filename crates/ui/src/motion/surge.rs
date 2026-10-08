use crate::view::Overview;

pub const MIN_SURGE_TOKENS: u64 = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct Surge {
    pub id: u64,
    pub delta: u64,
    pub at: i64,
}

fn agents_with_numbers(o: &Overview) -> String {
    let mut ids: Vec<String> = o.agents
        .iter()
        .filter(|a| a.enabled && a.totals.is_some())
        .map(|a| a.id.clone())
        .collect();
    ids.sort();
    ids.join(",")
}

#[derive(Default)]
pub struct SurgeDetector {
    pub surge_id: u64,
}

impl SurgeDetector {
    pub fn new() -> Self {
        Self { surge_id: 0 }
    }

    pub fn detect_surge(&mut self, prev: Option<&Overview>, next: &Overview, now_ms: i64) -> Option<Surge> {
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
        if agents_with_numbers(prev) != agents_with_numbers(next) {
            return None;
        }
        
        let prev_total = prev_totals.total;
        let next_total = next_totals.total;
        
        if next_total < prev_total {
            return None; // should not happen
        }
        
        let delta = next_total - prev_total;
        if delta >= MIN_SURGE_TOKENS {
            self.surge_id += 1;
            Some(Surge {
                id: self.surge_id,
                delta,
                at: now_ms,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Spark,
    Flare,
    Blast,
    Explosion,
    Inferno,
}

const LOG_FLOOR: f64 = 2.7;
const LOG_SPAN: f64 = 4.3;

pub fn surge_magnitude(delta: u64) -> f64 {
    if delta == 0 {
        return 0.0;
    }
    let m = ((delta as f64).log10() - LOG_FLOOR) / LOG_SPAN;
    m.clamp(0.0, 1.0)
}

pub fn blast_tier(delta: u64) -> Tier {
    let m = surge_magnitude(delta);
    if m < 0.25 { Tier::Spark }
    else if m < 0.45 { Tier::Flare }
    else if m < 0.7 { Tier::Blast }
    else if m < 0.93 { Tier::Explosion }
    else { Tier::Inferno }
}
