//! Decoding of Antigravity `gen_metadata` blobs into token counts.
//!
//! Layout (field numbers read from the protobuf descriptors embedded in `agy.exe` 1.3.1 — see
//! docs/COLLECTORS.md; validated on 5,434 real generations):
//!
//! ```text
//! CortexStepGeneratorMetadata { 1: chat_model = ChatModelMetadata, 2: step_indices (repeated uint32) }
//! ChatModelMetadata  { 4: usage = ModelUsageStats, 9: chat_start_metadata, 19: response_model, 21: model_display_name }
//! ChatStartMetadata  { 4: created_at = Timestamp{1: seconds} }
//! ModelUsageStats    { 2: input_tokens, 3: output_tokens, 4: cache_write_tokens, 5: cache_read_tokens,
//!                      9: thinking_output_tokens, 10: response_output_tokens, 11: response_id }
//! CortexStepMetadata (steps.metadata) { 1: created_at = Timestamp, 7/8/32: other Timestamps }
//! ```
//! Field 17 (`retry_infos`) repeats the usage of each attempt and is deliberately ignored.

use super::wire::{self, Message, WireError};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Generation {
    pub response_id: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub thinking: u64,
    pub response_output: u64,
    pub model: Option<String>,
    /// `chat_start_metadata.created_at` (seconds), when the agent version recorded it.
    pub created_secs: Option<i64>,
    /// First entry of `step_indices`: used to look up a timestamp when `created_secs` is absent.
    pub step_index: Option<u32>,
}

impl Generation {
    pub fn total(&self) -> u64 {
        self.input.saturating_add(self.output).saturating_add(self.cache_read).saturating_add(self.cache_write)
    }
}

/// `Ok(None)` = valid protobuf that simply is not a usage record. `Err` = not protobuf at all.
pub fn decode_generation(blob: &[u8]) -> Result<Option<Generation>, WireError> {
    let top = wire::parse(blob)?;
    let Some(chat) = top.message(1) else { return Ok(None) };
    let Some(usage) = chat.message(4) else { return Ok(None) };

    let model = chat
        .string(19)
        .filter(|s| !s.is_empty())
        .or_else(|| chat.string(21).filter(|s| !s.is_empty()))
        .map(str::to_string);
    let created_secs = chat.message(9).and_then(|m| m.message(4)).and_then(|t| t.varint(1)).and_then(|s| i64::try_from(s).ok());
    let step_index = top.repeated_varints(2).first().and_then(|v| u32::try_from(*v).ok());

    Ok(Some(Generation {
        response_id: usage.string(11).filter(|s| !s.is_empty()).map(str::to_string),
        input: usage.varint(2).unwrap_or(0),
        output: usage.varint(3).unwrap_or(0),
        cache_write: usage.varint(4).unwrap_or(0),
        cache_read: usage.varint(5).unwrap_or(0),
        thinking: usage.varint(9).unwrap_or(0),
        response_output: usage.varint(10).unwrap_or(0),
        model,
        created_secs,
        step_index,
    }))
}

/// Seconds from a `CortexStepMetadata` blob (`steps.metadata`), trying created_at then the other timestamps
/// the descriptor defines (finished_generating_at, completed_at, started_at).
pub fn step_timestamp(blob: &[u8]) -> Option<i64> {
    let m: Message<'_> = wire::parse(blob).ok()?;
    [1u32, 7, 8, 32]
        .iter()
        .find_map(|n| m.message(*n).and_then(|t| t.varint(1)).filter(|s| *s > 0).and_then(|s| i64::try_from(s).ok()))
}

/// Builders for synthetic blobs in the documented layout (shared with the collector tests).
#[cfg(test)]
pub mod fixture {
    use super::super::wire::enc::*;
    use super::*;

    /// Build a gen_metadata blob exactly as documented.
    pub fn blob(u: &Generation, with_start_time: bool) -> Vec<u8> {
        let mut usage = vec![v(1, 1020), v(2, u.input), v(3, u.output), v(4, u.cache_write), v(5, u.cache_read), v(6, 24), v(9, u.thinking), v(10, u.response_output)];
        if let Some(id) = &u.response_id {
            usage.push(s(11, id));
        }
        let mut chat = vec![v(3, 1020), msg(4, &usage)];
        if with_start_time {
            if let Some(t) = u.created_secs {
                chat.push(msg(9, &[v(1, 1), msg(4, &[v(1, t as u64), v(2, 123_456_789)])]));
            }
        }
        if let Some(m) = &u.model {
            chat.push(s(19, m));
        }
        // a retry_infos entry repeating the usage must never be counted twice
        chat.push(msg(17, &[msg(2, &usage)]));
        let mut top = vec![msg(1, &chat), s(4, "execution-id")];
        if let Some(step) = u.step_index {
            top.insert(1, bytes(2, &varint(u64::from(step))));
        }
        top.concat()
    }

    pub fn sample() -> Generation {
        Generation {
            response_id: Some("resp-abc123".into()),
            input: 2_049,
            output: 24,
            cache_write: 0,
            cache_read: 16_263,
            thinking: 10,
            response_output: 14,
            model: Some("gemini-pro-default".into()),
            created_secs: Some(1_782_892_239),
            step_index: Some(7),
        }
    }

}

#[cfg(test)]
mod tests {
    use super::super::wire::enc::*;
    use super::fixture::{blob, sample};
    use super::*;

    #[test]
    fn decodes_every_documented_field() {
        let g = decode_generation(&blob(&sample(), true)).unwrap().unwrap();
        assert_eq!(g, sample());
        assert_eq!(g.total(), 2_049 + 24 + 16_263);
    }

    #[test]
    fn output_is_thinking_plus_response_in_real_data_and_we_keep_output() {
        let g = decode_generation(&blob(&sample(), true)).unwrap().unwrap();
        assert_eq!(g.output, g.thinking + g.response_output);
    }

    #[test]
    fn the_retry_info_copy_of_usage_is_not_double_counted() {
        // blob() embeds a field-17 copy of the usage; decoding must read field 4 only.
        let g = decode_generation(&blob(&sample(), true)).unwrap().unwrap();
        assert_eq!(g.input, 2_049);
    }

    #[test]
    fn rows_without_a_start_time_still_expose_the_step_index_for_the_fallback() {
        let g = decode_generation(&blob(&sample(), false)).unwrap().unwrap();
        assert_eq!(g.created_secs, None);
        assert_eq!(g.step_index, Some(7));
    }

    #[test]
    fn the_model_falls_back_to_the_display_name() {
        let mut g = sample();
        g.model = None;
        let usage = [v(2, 5), v(3, 1), s(11, "r")].concat();
        let blob = msg(1, &[msg(4, &[usage]), s(21, "Gemini 3.1 Pro (High)")]);
        let d = decode_generation(&blob).unwrap().unwrap();
        assert_eq!(d.model.as_deref(), Some("Gemini 3.1 Pro (High)"));
    }

    #[test]
    fn valid_protobuf_without_a_usage_block_is_not_an_error() {
        assert_eq!(decode_generation(&[s(4, "execution-id")].concat()).unwrap(), None);
        assert_eq!(decode_generation(&msg(1, &[v(3, 1020)])).unwrap(), None);
        assert_eq!(decode_generation(&[]).unwrap(), None);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode_generation(&[0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn step_timestamps_come_from_created_at_then_the_other_documented_times() {
        assert_eq!(step_timestamp(&msg(1, &[v(1, 1_790_000_000)])), Some(1_790_000_000));
        assert_eq!(step_timestamp(&msg(7, &[v(1, 1_790_000_100)])), Some(1_790_000_100)); // no created_at
        assert_eq!(step_timestamp(&msg(1, &[v(1, 0)])), None, "a zero timestamp is no timestamp");
        assert_eq!(step_timestamp(&[0xff]), None);
    }
}
