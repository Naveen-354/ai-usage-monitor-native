//! Merges repeated records for the same upstream response before they are stored.
//!
//! Several agents write one API response on many lines (one per content block) or revise a streaming
//! partial into a final record. The rule everywhere is the same: keep the **per-field maximum**.

use std::collections::HashMap;

use crate::model::UsageEvent;

#[derive(Default)]
pub struct Accumulator {
    events: Vec<UsageEvent>,
    index: HashMap<String, usize>,
}

impl Accumulator {
    pub fn push(&mut self, e: UsageEvent) {
        match self.index.get(&e.dedupe_key) {
            Some(&i) => {
                let cur = &mut self.events[i];
                cur.input_tokens = cur.input_tokens.max(e.input_tokens);
                cur.output_tokens = cur.output_tokens.max(e.output_tokens);
                cur.cache_read_tokens = cur.cache_read_tokens.max(e.cache_read_tokens);
                cur.cache_write_tokens = cur.cache_write_tokens.max(e.cache_write_tokens);
                cur.reasoning_tokens = match (cur.reasoning_tokens, e.reasoning_tokens) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
                if cur.project.is_none() {
                    cur.project = e.project;
                }
                if cur.session_id.is_none() {
                    cur.session_id = e.session_id;
                }
            }
            None => {
                self.index.insert(e.dedupe_key.clone(), self.events.len());
                self.events.push(e);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn into_events(self) -> Vec<UsageEvent> {
        self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Accuracy;

    fn ev(key: &str, input: u64, output: u64, reasoning: Option<u64>) -> UsageEvent {
        UsageEvent {
            agent: "claude",
            model: "m".into(),
            ts_utc_ms: 1,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: reasoning,
            session_id: None,
            project: None,
            source: "t",
            accuracy: Accuracy::Real,
            dedupe_key: key.into(),
        }
    }

    #[test]
    fn repeats_collapse_to_the_per_field_maximum() {
        let mut a = Accumulator::default();
        a.push(ev("k", 2, 10, None));
        a.push(ev("k", 2, 300, Some(40)));
        a.push(ev("k", 1, 50, Some(7)));
        let out = a.into_events();
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].input_tokens, out[0].output_tokens, out[0].reasoning_tokens), (2, 300, Some(40)));
    }

    #[test]
    fn distinct_keys_stay_distinct_and_keep_order() {
        let mut a = Accumulator::default();
        a.push(ev("a", 1, 1, None));
        a.push(ev("b", 2, 2, None));
        a.push(ev("a", 5, 5, None));
        let out = a.into_events();
        assert_eq!(out.iter().map(|e| e.dedupe_key.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(out[0].input_tokens, 5);
    }

    #[test]
    fn unknown_reasoning_is_filled_in_when_a_later_record_knows_it() {
        let mut a = Accumulator::default();
        a.push(ev("k", 1, 1, None));
        a.push(ev("k", 1, 1, Some(9)));
        assert_eq!(a.into_events()[0].reasoning_tokens, Some(9));
    }
}
