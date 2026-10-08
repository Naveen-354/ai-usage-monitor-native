use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::*;
use crate::collectors::testkit::{cursors_from, DbSink, Fixture, VecSink};
use crate::collectors::{Cursor, ProjectResolver};
use crate::database::testutil::temp_db;

const SID: &str = "019ff4ba-93df-7ad2-b79e-e2671e816506";

fn meta(cwd: &str) -> String {
    json!({"timestamp":"2026-08-12T06:48:17.308Z","type":"session_meta","payload":{"id":SID,"session_id":SID,"cwd":cwd,"originator":"Codex Desktop","cli_version":"0.147.0"}}).to_string()
}

fn turn(model: &str, ts: &str) -> String {
    json!({"timestamp":ts,"type":"turn_context","payload":{"turn_id":"t","cwd":"","model":model}}).to_string()
}

fn count(ts: &str, input: u64, cached: u64, output: u64, reasoning: u64) -> String {
    json!({"timestamp":ts,"type":"event_msg","payload":{"type":"token_count","info":{
        "total_token_usage":{"input_tokens":input,"cached_input_tokens":cached,"cache_write_input_tokens":0,
            "output_tokens":output,"reasoning_output_tokens":reasoning,"total_tokens":input+output},
        "last_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2},
        "model_context_window":258400},
        "rate_limits":{"plan_type":"pro"}}})
    .to_string()
}

fn rate_limit_only(ts: &str) -> String {
    json!({"timestamp":ts,"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"plan_type":"pro"}}}).to_string()
}

fn chatter() -> String {
    json!({"timestamp":"2026-08-12T06:48:20.000Z","type":"event_msg","payload":{"type":"agent_message","message":"SECRET PROMPT TEXT must never be stored"}}).to_string()
}

fn write_lines(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::File::create(path).unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
}

fn append(path: &Path, text: &str) {
    std::fs::OpenOptions::new().append(true).open(path).unwrap().write_all(text.as_bytes()).unwrap();
}

struct Setup {
    d: tempfile::TempDir,
    fx: Fixture,
    file: PathBuf,
}

fn setup() -> Setup {
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join(".codex").join("sessions").join("2026").join("08").join("12").join(format!("rollout-2026-08-12T12-18-16-{SID}.jsonl"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let fx = Fixture::new(d.path());
    Setup { d, fx, file }
}

fn run(s: &Setup, cursors: &HashMap<String, Cursor>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
    let det = CodexCollector.detect(&s.fx.env);
    CodexCollector.collect_usage(&s.fx.ctx(&det, cursors), sink)
}

fn events_of(lines: &[String]) -> Vec<UsageEvent> {
    let s = setup();
    write_lines(&s.file, lines);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    sink.events
}

fn total_of(events: &[UsageEvent]) -> u64 {
    events.iter().map(|e| e.total_tokens()).sum()
}

#[test]
fn each_event_is_the_delta_of_the_cumulative_total() {
    let ev = events_of(&[
        meta(""),
        turn("gpt-5.6-luna", "2026-08-12T06:48:18.993Z"),
        count("2026-08-12T06:48:35.300Z", 16_631, 9_984, 184, 56),
        count("2026-08-12T06:48:41.965Z", 36_449, 26_112, 425, 185),
    ]);
    assert_eq!(ev.len(), 2);
    // first: whole cumulative; fresh input = 16631 - 9984
    assert_eq!((ev[0].input_tokens, ev[0].cache_read_tokens, ev[0].output_tokens, ev[0].reasoning_tokens), (6_647, 9_984, 184, Some(56)));
    // second: 36449-16631 = 19818 input, cached 16128 → fresh 3690; output 241; reasoning 129
    assert_eq!((ev[1].input_tokens, ev[1].cache_read_tokens, ev[1].output_tokens, ev[1].reasoning_tokens), (3_690, 16_128, 241, Some(129)));
    assert_eq!(ev[0].model, "gpt-5.6-luna");
    assert_eq!(ev[0].session_id.as_deref(), Some(SID));
    assert_eq!(ev[0].accuracy, Accuracy::Real);
}

#[test]
fn the_sum_of_deltas_equals_codexs_own_final_total() {
    let ev = events_of(&[
        meta(""),
        turn("m", "2026-08-12T06:48:18.993Z"),
        count("2026-08-12T06:48:35.300Z", 16_631, 9_984, 184, 56),
        count("2026-08-12T06:48:41.965Z", 36_449, 26_112, 425, 185),
        count("2026-08-12T06:48:50.580Z", 56_532, 45_312, 721, 210),
    ]);
    assert_eq!(total_of(&ev), 56_532 + 721, "must equal total_tokens (= input + output) of the last event");
}

#[test]
fn repeated_totals_and_rate_limit_only_updates_add_nothing() {
    let ev = events_of(&[
        meta(""),
        turn("m", "2026-08-12T06:48:18.993Z"),
        count("2026-08-12T06:48:35.300Z", 1_000, 100, 50, 5),
        rate_limit_only("2026-08-12T06:48:36.000Z"),
        count("2026-08-12T06:48:37.000Z", 1_000, 100, 50, 5), // same total again
    ]);
    assert_eq!(ev.len(), 1);
    assert_eq!(total_of(&ev), 1_050);
}

#[test]
fn a_counter_reset_starts_a_new_baseline_without_key_collisions() {
    let ev = events_of(&[
        meta(""),
        turn("m", "2026-08-12T06:48:18.993Z"),
        count("2026-08-12T06:48:35.000Z", 5_000, 0, 500, 0), // total 5500
        count("2026-08-12T06:49:00.000Z", 2_000, 0, 200, 0), // went backwards (compaction): total 2200
        count("2026-08-12T06:50:00.000Z", 5_000, 0, 500, 0), // climbs back through 5500 again
    ]);
    assert_eq!(ev.len(), 3);
    assert_eq!(total_of(&ev), 5_500 + 2_200 + (5_500 - 2_200));
    let keys: std::collections::HashSet<_> = ev.iter().map(|e| &e.dedupe_key).collect();
    assert_eq!(keys.len(), 3, "a total that recurs after a reset must not reuse an earlier key");
}

#[test]
fn the_model_follows_turn_context_changes() {
    let ev = events_of(&[
        meta(""),
        turn("model-a", "2026-08-12T06:48:18.000Z"),
        count("2026-08-12T06:48:35.000Z", 1_000, 0, 10, 0),
        turn("model-b", "2026-08-12T06:49:00.000Z"),
        count("2026-08-12T06:49:30.000Z", 3_000, 0, 30, 0),
    ]);
    assert_eq!((ev[0].model.as_str(), ev[1].model.as_str()), ("model-a", "model-b"));
}

#[test]
fn usage_before_any_turn_context_gets_an_unknown_model_not_a_guess() {
    let ev = events_of(&[meta(""), count("2026-08-12T06:48:35.000Z", 100, 0, 10, 0)]);
    assert_eq!(ev[0].model, "unknown");
}

#[test]
fn the_session_id_falls_back_to_the_file_name() {
    let ev = events_of(&[count("2026-08-12T06:48:35.000Z", 100, 0, 10, 0)]);
    assert_eq!(ev[0].session_id.as_deref(), Some(SID));
    assert!(ev[0].dedupe_key.starts_with(&format!("codex:{SID}:")));
}

#[test]
fn a_copied_parent_header_does_not_relabel_the_session() {
    // Forked rollouts embed the parent thread's session_meta; events must stay on this file's own session.
    let parent = json!({"timestamp":"2026-08-12T06:48:19.000Z","type":"session_meta","payload":{"id":"parent-thread-id","cwd":"/somewhere/else"}}).to_string();
    let ev = events_of(&[
        meta(""),
        turn("m", "2026-08-12T06:48:18.000Z"),
        count("2026-08-12T06:48:35.000Z", 1_000, 0, 10, 0),
        parent,
        count("2026-08-12T06:48:40.000Z", 3_000, 0, 30, 0),
    ]);
    assert_eq!(ev.len(), 2);
    assert!(ev.iter().all(|e| e.session_id.as_deref() == Some(SID)), "{:?}", ev.iter().map(|e| &e.session_id).collect::<Vec<_>>());
    assert!(ev.iter().all(|e| e.dedupe_key.starts_with(&format!("codex:{SID}:"))));
}

#[test]
fn a_cwd_in_the_header_becomes_the_project() {
    let s = setup();
    let work = s.d.path().join("work").join("agent-monitor");
    std::fs::create_dir_all(work.join(".git")).unwrap();
    write_lines(&s.file, &[meta(&work.to_string_lossy()), turn("m", "2026-08-12T06:48:18.000Z"), count("2026-08-12T06:48:35.000Z", 100, 0, 10, 0)]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events[0].project.as_ref().unwrap().name, "agent-monitor");
}

#[test]
fn tailing_across_runs_continues_the_running_total_instead_of_recounting() {
    let s = setup();
    write_lines(&s.file, &[meta(""), turn("m", "2026-08-12T06:48:18.000Z"), count("2026-08-12T06:48:35.000Z", 10_000, 4_000, 100, 10)]);
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    run(&s, &HashMap::new(), &mut sink).unwrap();

    append(&s.file, &format!("{}\n", count("2026-08-12T06:49:35.000Z", 25_000, 14_000, 400, 30)));
    let cursors = db.with_reader(|c| crate::database::queries::load_cursors(c, "codex")).unwrap();
    run(&s, &cursors, &mut sink).unwrap();

    let totals = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap();
    let t = totals["codex"];
    assert_eq!(t.events, 2);
    assert_eq!(t.total_all(), 25_000 + 400, "Σ events must equal the final cumulative total, not total + total");
    assert_eq!((t.input, t.cache_read, t.output), (11_000, 14_000, 400));
}

#[test]
fn rereading_a_file_from_scratch_is_idempotent() {
    let s = setup();
    write_lines(&s.file, &[meta(""), turn("m", "2026-08-12T06:48:18.000Z"), count("2026-08-12T06:48:35.000Z", 1_000, 0, 10, 0), count("2026-08-12T06:48:40.000Z", 3_000, 0, 30, 0)]);
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    run(&s, &HashMap::new(), &mut sink).unwrap();
    run(&s, &HashMap::new(), &mut sink).unwrap(); // no cursors: a full re-read
    let totals = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap();
    assert_eq!(totals["codex"].total_all(), 3_030);
    assert_eq!(totals["codex"].events, 2);
}

#[test]
fn a_shrunk_file_restarts_with_fresh_state() {
    let s = setup();
    write_lines(&s.file, &[meta(""), turn("m", "2026-08-12T06:48:18.000Z"), count("2026-08-12T06:48:35.000Z", 9_000, 0, 90, 0), count("2026-08-12T06:48:40.000Z", 19_000, 0, 190, 0)]);
    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    write_lines(&s.file, &[count("2026-08-12T07:00:00.000Z", 500, 0, 5, 0)]);
    let mut second = VecSink::default();
    run(&s, &cursors_from(&first.cursors), &mut second).unwrap();
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0].total_tokens(), 505, "no stale previous total may be subtracted");
}

#[test]
fn a_half_written_line_is_retried_when_complete() {
    let s = setup();
    write_lines(&s.file, &[meta(""), turn("m", "2026-08-12T06:48:18.000Z")]);
    let full = count("2026-08-12T06:48:35.000Z", 1_000, 0, 10, 0);
    append(&s.file, &full[..full.len() / 2]);
    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    assert!(first.events.is_empty());

    append(&s.file, &format!("{}\n", &full[full.len() / 2..]));
    let mut second = VecSink::default();
    run(&s, &cursors_from(&first.cursors), &mut second).unwrap();
    assert_eq!(second.events.len(), 1);
}

#[test]
fn unchanged_files_are_skipped_and_conversation_text_is_never_kept() {
    let s = setup();
    write_lines(&s.file, &[meta(""), chatter(), count("2026-08-12T06:48:35.000Z", 1_000, 0, 10, 0)]);
    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    assert!(!format!("{:?}{:?}", first.events, first.cursors).contains("SECRET PROMPT TEXT"));
    let mut second = VecSink::default();
    let sum = run(&s, &cursors_from(&first.cursors), &mut second).unwrap();
    assert_eq!((sum.files_seen, sum.files_read), (1, 0));
}

#[test]
fn files_that_are_not_rollouts_are_ignored() {
    let s = setup();
    write_lines(&s.file.with_file_name("notes.jsonl"), &[count("2026-08-12T06:48:35.000Z", 1_000, 0, 10, 0)]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert!(sink.events.is_empty());
}

#[test]
fn uuid_is_extracted_from_rollout_names() {
    assert_eq!(uuid_from_file_name(Path::new("rollout-2026-02-10T22-15-57-019c4872-03c8-7ce1-a56a-1562b40e002d.jsonl")).as_deref(), Some("019c4872-03c8-7ce1-a56a-1562b40e002d"));
    assert_eq!(uuid_from_file_name(Path::new("other.jsonl")), None);
}

#[test]
fn detection_distinguishes_installed_data_only_and_absent() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(CodexCollector.detect(&Env::with_home(d.path())).presence, Presence::NotFound);
    std::fs::create_dir_all(d.path().join(".codex").join("sessions")).unwrap();
    assert_eq!(CodexCollector.detect(&Env::with_home(d.path())).presence, Presence::DataOnly);
    let bin = d.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(if cfg!(windows) { "codex.cmd" } else { "codex" }), b"").unwrap();
    let mut env = Env::with_home(d.path());
    env.path_dirs = vec![bin];
    assert_eq!(CodexCollector.detect(&env).presence, Presence::Installed);
}

// ---------------------------------------------------------------------------------------------------
// Real-data regression (opt-in): `cargo test codex::tests::real -- --ignored --nocapture`
// ---------------------------------------------------------------------------------------------------
#[test]
#[ignore = "reads the real ~/.codex directory"]
fn real_data_final_totals_match_per_session() {
    let env = Env::from_system();
    let root = sessions_dir(&env);
    if !root.is_dir() {
        eprintln!("no ~/.codex/sessions here; nothing to verify");
        return;
    }

    // Reference (untyped parse): per rollout, replay the cumulative total with the same reset rule,
    // and remember each session's final cumulative total when no reset occurred.
    let mut ref_total: u64 = 0;
    let mut ref_sessions: HashMap<String, u64> = HashMap::new();
    for (path, _) in walk::files_with_suffix(&root, ".jsonl", MAX_DEPTH) {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Some(sid) = uuid_from_file_name(&path) else { continue };
        let (mut prev, mut sum, mut resets) = (None::<u64>, 0u64, 0);
        for line in text.lines() {
            if !line.contains("\"token_count\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let Some(t) = v["payload"]["info"]["total_token_usage"]["total_tokens"].as_u64() else { continue };
            match prev {
                Some(p) if t == p => continue,
                Some(p) if t < p => {
                    resets += 1;
                    sum += t;
                }
                Some(p) => sum += t - p,
                None => sum += t,
            }
            prev = Some(t);
        }
        ref_total += sum;
        if resets == 0 {
            ref_sessions.insert(sid, sum);
        }
    }

    let (_d, db) = temp_db();
    let det = CodexCollector.detect(&env);
    let fx = Fixture::new(&env.home);
    let projects = ProjectResolver::new(true, env.home.clone());
    let cursors = HashMap::new();
    let ctx = CollectCtx { env: &env, detection: &det, cursors: &cursors, projects: &projects, cancel: &fx.cancel };
    let mut sink = DbSink { db: db.clone(), agent: ID };
    let sum = CodexCollector.collect_usage(&ctx, &mut sink).unwrap();

    let totals = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap();
    let t = totals["codex"];
    let per_session: HashMap<String, u64> = db
        .with_reader(|c| {
            let mut st = c.prepare("SELECT s.external_id, SUM(e.input_tokens + e.output_tokens + e.cache_read_tokens + e.cache_write_tokens) FROM usage_events e JOIN sessions s ON s.id = e.session_id GROUP BY s.external_id")?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?;
            Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
        })
        .unwrap();

    eprintln!("reference Σ deltas: {ref_total}; collector: {}; events {}; skipped {}", t.total_all(), t.events, sum.skipped_records);
    // A session with no usage events has no row at all: that is a zero, not a mismatch.
    let mismatched: Vec<_> = ref_sessions
        .iter()
        .filter_map(|(sid, want)| {
            let got = per_session.get(sid).copied().unwrap_or(0);
            (got != *want).then(|| (sid.clone(), *want, got))
        })
        .collect();
    eprintln!("sessions compared: {}; mismatched: {}", ref_sessions.len(), mismatched.len());
    assert!(mismatched.is_empty(), "per-session totals differ (sid, reference, collector): {mismatched:?}");
    let drift = t.total_all().abs_diff(ref_total) as f64 / ref_total.max(1) as f64;
    assert!(drift < 0.001, "overall total {} vs reference {ref_total}", t.total_all());
}
