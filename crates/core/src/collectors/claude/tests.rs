use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use serde_json::json;

use super::*;
use crate::collectors::testkit::{cursors_from, DbSink, Fixture, VecSink};
use crate::collectors::{Cursor, ProjectResolver};
use crate::database::testutil::temp_db;

const TS: &str = "2026-10-07T05:27:49.245Z";
const TS_MS: i64 = 1_791_350_869_245;

struct Rec<'a> {
    id: &'a str,
    ts: &'a str,
    model: &'a str,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    thinking: Option<u64>,
    session: &'a str,
    cwd: &'a str,
}

impl<'a> Rec<'a> {
    fn new(id: &'a str) -> Rec<'a> {
        Rec { id, ts: TS, model: "claude-opus-5", input: 2, output: 309, cache_read: 59_613, cache_write: 1_022, thinking: Some(52), session: "sess-1", cwd: "" }
    }
    fn json(&self) -> String {
        let mut usage = json!({
            "input_tokens": self.input, "output_tokens": self.output,
            "cache_read_input_tokens": self.cache_read, "cache_creation_input_tokens": self.cache_write,
            "service_tier": "standard",
        });
        if let Some(t) = self.thinking {
            usage["output_tokens_details"] = json!({ "thinking_tokens": t });
        }
        json!({
            "type": "assistant", "timestamp": self.ts, "sessionId": self.session, "cwd": self.cwd,
            "requestId": format!("req-{}", self.id), "uuid": "u",
            "message": { "id": self.id, "model": self.model, "role": "assistant",
                         "content": [{"type": "text", "text": "SECRET PROMPT TEXT must never be stored"}], "usage": usage }
        })
        .to_string()
    }
}

fn user_line_mentioning_usage() -> String {
    json!({"type": "user", "timestamp": TS, "message": {"role": "user", "content": "please explain the word \"usage\""}}).to_string()
}

fn write_lines(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::File::create(path).unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
}

fn append(path: &Path, text: &str) {
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

struct Setup {
    _d: tempfile::TempDir,
    fx: Fixture,
    root: PathBuf,
}

fn setup() -> Setup {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join(".claude").join("projects");
    std::fs::create_dir_all(&root).unwrap();
    let fx = Fixture::new(d.path());
    Setup { _d: d, fx, root }
}

fn run(s: &Setup, cursors: &HashMap<String, Cursor>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
    let det = ClaudeCollector.detect(&s.fx.env);
    ClaudeCollector.collect_usage(&s.fx.ctx(&det, cursors), sink)
}

#[test]
fn maps_usage_fields_into_disjoint_counters() {
    let s = setup();
    write_lines(&s.root.join("D--proj").join("a.jsonl"), &[Rec::new("msg_1").json()]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    let e = &sink.events[0];
    assert_eq!(e.agent, "claude");
    assert_eq!(e.model, "claude-opus-5");
    assert_eq!(e.ts_utc_ms, TS_MS);
    assert_eq!((e.input_tokens, e.output_tokens, e.cache_read_tokens, e.cache_write_tokens), (2, 309, 59_613, 1_022));
    assert_eq!(e.reasoning_tokens, Some(52));
    assert_eq!(e.total_tokens(), 2 + 309 + 59_613 + 1_022);
    assert_eq!(e.session_id.as_deref(), Some("sess-1"));
    assert_eq!(e.dedupe_key, "claude:msg_1");
    assert_eq!(e.accuracy, Accuracy::Real);
    assert_eq!(sum.last_event_ms, Some(TS_MS));
}

#[test]
fn missing_thinking_tokens_stay_unknown_not_zero() {
    let s = setup();
    let mut r = Rec::new("m");
    r.thinking = None;
    write_lines(&s.root.join("p").join("a.jsonl"), &[r.json()]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events[0].reasoning_tokens, None);
}

#[test]
fn one_response_logged_on_three_lines_is_counted_once() {
    let s = setup();
    let rec = Rec::new("msg_dup");
    write_lines(&s.root.join("p").join("a.jsonl"), &[rec.json(), rec.json(), rec.json()]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1, "duplicate content-block lines must collapse");
}

#[test]
fn a_streaming_partial_followed_by_the_final_record_keeps_the_maximum() {
    let s = setup();
    let mut partial = Rec::new("m");
    partial.output = 12;
    let mut fin = Rec::new("m");
    fin.output = 900;
    write_lines(&s.root.join("p").join("a.jsonl"), &[partial.json(), fin.json()]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sink.events[0].output_tokens, 900);
}

#[test]
fn the_same_response_in_two_files_is_stored_once_in_the_database() {
    // A resumed session copies earlier history into a new file: 234 ids did this on the author's machine.
    let s = setup();
    write_lines(&s.root.join("p").join("old.jsonl"), &[Rec::new("shared").json(), Rec::new("only-old").json()]);
    write_lines(&s.root.join("p").join("resumed.jsonl"), &[Rec::new("shared").json(), Rec::new("only-new").json()]);
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    run(&s, &HashMap::new(), &mut sink).unwrap();
    let (count, _) = db.with_reader(|c| crate::database::queries::agent_event_stats(c, "claude")).unwrap();
    assert_eq!(count, 3, "shared + only-old + only-new");
    let totals = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap();
    assert_eq!(totals["claude"].events, 3);
    assert_eq!(totals["claude"].output, 309 * 3);
}

#[test]
fn synthetic_and_zero_usage_records_are_skipped_but_counted() {
    let s = setup();
    let mut synthetic = Rec::new("syn");
    synthetic.model = "<synthetic>";
    let mut zero = Rec::new("zero");
    (zero.input, zero.output, zero.cache_read, zero.cache_write) = (0, 0, 0, 0);
    write_lines(&s.root.join("p").join("a.jsonl"), &[synthetic.json(), zero.json(), Rec::new("real").json()]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sum.skipped_records, 2);
}

#[test]
fn records_without_a_timestamp_or_a_dedupe_key_are_skipped_never_guessed() {
    let s = setup();
    let mut undated = Rec::new("undated");
    undated.ts = "not-a-time";
    let keyless = json!({"type":"assistant","timestamp":TS,"message":{"model":"m","usage":{"input_tokens":5,"output_tokens":5}}}).to_string();
    write_lines(&s.root.join("p").join("a.jsonl"), &[undated.json(), keyless]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert!(sink.events.is_empty());
    assert_eq!(sum.skipped_records, 2);
}

#[test]
fn non_assistant_lines_that_merely_mention_usage_are_ignored() {
    let s = setup();
    write_lines(&s.root.join("p").join("a.jsonl"), &[user_line_mentioning_usage(), Rec::new("m").json()]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sum.skipped_records, 0, "an ordinary user line is not a skipped usage record");
}

#[test]
fn prompt_text_never_reaches_the_events() {
    let s = setup();
    write_lines(&s.root.join("p").join("a.jsonl"), &[Rec::new("m").json()]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    let dump = format!("{:?}", sink.events);
    assert!(!dump.contains("SECRET PROMPT TEXT"));
}

#[test]
fn the_working_directory_becomes_the_project() {
    let s = setup();
    let work = s._d.path().join("work").join("robot-simulator");
    std::fs::create_dir_all(work.join(".git")).unwrap();
    let mut r = Rec::new("m");
    let cwd = work.to_string_lossy().to_string();
    r.cwd = &cwd;
    write_lines(&s.root.join("p").join("a.jsonl"), &[r.json()]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events[0].project.as_ref().unwrap().name, "robot-simulator");
}

#[test]
fn a_later_run_reads_only_what_was_appended() {
    let s = setup();
    let file = s.root.join("p").join("a.jsonl");
    write_lines(&file, &[Rec::new("m1").json()]);
    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    assert_eq!(first.events.len(), 1);

    append(&file, &format!("{}\n", Rec::new("m2").json()));
    let cursors = cursors_from(&first.cursors);
    let mut second = VecSink::default();
    let sum = run(&s, &cursors, &mut second).unwrap();
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0].dedupe_key, "claude:m2", "must not re-read m1");
    assert_eq!(sum.files_read, 1);
}

#[test]
fn an_unchanged_file_is_not_opened_at_all() {
    let s = setup();
    write_lines(&s.root.join("p").join("a.jsonl"), &[Rec::new("m1").json()]);
    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    let mut second = VecSink::default();
    let sum = run(&s, &cursors_from(&first.cursors), &mut second).unwrap();
    assert_eq!((sum.files_seen, sum.files_read), (1, 0));
    assert!(second.cursors.is_empty() && second.events.is_empty());
}

#[test]
fn a_half_written_last_line_is_retried_once_complete() {
    let s = setup();
    let file = s.root.join("p").join("a.jsonl");
    let full = Rec::new("m2").json();
    write_lines(&file, &[Rec::new("m1").json()]);
    append(&file, &full[..full.len() / 2]); // writer is mid-line

    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    assert_eq!(first.events.len(), 1, "the half line must not be counted or lost");

    append(&file, &format!("{}\n", &full[full.len() / 2..]));
    let mut second = VecSink::default();
    run(&s, &cursors_from(&first.cursors), &mut second).unwrap();
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0].dedupe_key, "claude:m2");
}

#[test]
fn a_line_cut_before_the_usage_keyword_is_not_lost() {
    // Regression: the "usage" fast-path filter used to swallow a half-written line that had not reached
    // the keyword yet, permanently dropping that response.
    let s = setup();
    let file = s.root.join("p").join("a.jsonl");
    let full = Rec::new("m2").json();
    let cut = full.find("\"usage\"").unwrap() - 5;
    write_lines(&file, &[Rec::new("m1").json()]);
    append(&file, &full[..cut]);
    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    assert_eq!(first.events.len(), 1);

    append(&file, &format!("{}
", &full[cut..]));
    let mut second = VecSink::default();
    run(&s, &cursors_from(&first.cursors), &mut second).unwrap();
    assert_eq!(second.events.len(), 1, "the completed line must be picked up");
    assert_eq!(second.events[0].dedupe_key, "claude:m2");
}

#[test]
fn a_rewritten_shorter_file_is_re_read_without_double_counting() {
    let s = setup();
    let file = s.root.join("p").join("a.jsonl");
    write_lines(&file, &[Rec::new("m1").json(), Rec::new("m2").json(), Rec::new("m3").json()]);
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    run(&s, &HashMap::new(), &mut sink).unwrap();
    let cursors = db.with_reader(|c| crate::database::queries::load_cursors(c, "claude")).unwrap();

    write_lines(&file, &[Rec::new("m1").json()]); // compacted/rotated
    run(&s, &cursors, &mut sink).unwrap();
    let (count, _) = db.with_reader(|c| crate::database::queries::agent_event_stats(c, "claude")).unwrap();
    assert_eq!(count, 3, "history already stored must remain, and nothing is counted twice");
}

#[test]
fn nested_subagent_files_are_read() {
    let s = setup();
    write_lines(&s.root.join("p").join("session").join("subagents").join("agent-1.jsonl"), &[Rec::new("sub").json()]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
}

#[test]
fn a_malformed_terminated_line_is_counted_and_does_not_stop_the_run() {
    let s = setup();
    write_lines(&s.root.join("p").join("a.jsonl"), &["{\"usage\": broken".to_string(), Rec::new("ok").json()]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sum.skipped_records, 1);
}

#[test]
fn cancellation_stops_a_run_between_files() {
    let s = setup();
    write_lines(&s.root.join("p").join("a.jsonl"), &[Rec::new("m").json()]);
    s.fx.cancel.store(true, Ordering::Relaxed);
    let mut sink = VecSink::default();
    assert!(matches!(run(&s, &HashMap::new(), &mut sink), Err(CollectError::Cancelled)));
    assert!(sink.events.is_empty());
}

#[test]
fn detection_distinguishes_installed_data_only_and_absent() {
    let d = tempfile::tempdir().unwrap();
    let absent = Env::with_home(d.path());
    assert_eq!(ClaudeCollector.detect(&absent).presence, Presence::NotFound);

    std::fs::create_dir_all(d.path().join(".claude").join("projects")).unwrap();
    let data_only = ClaudeCollector.detect(&absent);
    assert_eq!(data_only.presence, Presence::DataOnly);
    assert!(data_only.note.unwrap().contains("not on PATH"));
    assert_eq!(data_only.roots.len(), 1);

    let bin = d.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(if cfg!(windows) { "claude.exe" } else { "claude" }), b"").unwrap();
    let mut env = Env::with_home(d.path());
    env.path_dirs = vec![bin];
    assert_eq!(ClaudeCollector.detect(&env).presence, Presence::Installed);
}

// ---------------------------------------------------------------------------------------------------
// Real-data regression (opt-in): `cargo test claude::tests::real -- --ignored --nocapture`
// Re-derives the answer with a *different* parser (untyped serde_json::Value) and compares.
// ---------------------------------------------------------------------------------------------------
#[test]
#[ignore = "reads the real ~/.claude directory"]
fn real_data_matches_an_independent_count() {
    let env = Env::from_system();
    let root = projects_dir(&env);
    if !root.is_dir() {
        eprintln!("no ~/.claude/projects here; nothing to verify");
        return;
    }

    // Independent reference implementation.
    let mut reference: HashMap<String, [u64; 4]> = HashMap::new();
    let mut raw_records = 0u64;
    for (path, _) in walk::files_with_suffix(&root, ".jsonl", MAX_DEPTH) {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            if v["type"] != "assistant" || !v["message"]["usage"].is_object() {
                continue;
            }
            raw_records += 1;
            let m = &v["message"];
            if m["model"] == "<synthetic>" || DateTime::parse_from_rfc3339(v["timestamp"].as_str().unwrap_or("")).is_err() {
                continue;
            }
            let Some(id) = m["id"].as_str() else { continue };
            let n = |k: &str| m["usage"][k].as_u64().unwrap_or(0);
            let cur = [n("input_tokens"), n("output_tokens"), n("cache_read_input_tokens"), n("cache_creation_input_tokens")];
            if cur.iter().sum::<u64>() == 0 {
                continue;
            }
            let e = reference.entry(id.to_string()).or_insert([0; 4]);
            for i in 0..4 {
                e[i] = e[i].max(cur[i]);
            }
        }
    }

    // The collector, into a real (temp) database.
    let (_d, db) = temp_db();
    let det = ClaudeCollector.detect(&env);
    let fx = Fixture::new(&env.home);
    let projects = ProjectResolver::new(true, env.home.clone());
    let cursors = HashMap::new();
    let ctx = CollectCtx { env: &env, detection: &det, cursors: &cursors, projects: &projects, cancel: &fx.cancel };
    let mut sink = DbSink { db: db.clone(), agent: ID };
    let sum = ClaudeCollector.collect_usage(&ctx, &mut sink).unwrap();

    let (count, _) = db.with_reader(|c| crate::database::queries::agent_event_stats(c, "claude")).unwrap();
    let totals = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap();
    let t = totals["claude"];
    let ref_sum: [u64; 4] = reference.values().fold([0; 4], |mut a, e| {
        for i in 0..4 {
            a[i] += e[i];
        }
        a
    });
    eprintln!("raw assistant records: {raw_records}; unique responses (reference): {}; collector events: {count}", reference.len());
    eprintln!("reference sums [in,out,cacheR,cacheW]: {ref_sum:?}; collector: [{},{},{},{}]; skipped {}", t.input, t.output, t.cache_read, t.cache_write, sum.skipped_records);

    // The active session file keeps growing while we read it twice, so allow a small drift.
    let drift = |a: u64, b: u64| a.abs_diff(b) as f64 / (b.max(1) as f64);
    assert!(drift(count, reference.len() as u64) < 0.005, "event count {count} vs {}", reference.len());
    for (i, (got, want)) in [t.input, t.output, t.cache_read, t.cache_write].iter().zip(ref_sum).enumerate() {
        assert!(drift(*got, want) < 0.005, "field {i}: {got} vs {want}");
    }
    assert!(count < raw_records, "de-duplication must reduce the record count");
}
