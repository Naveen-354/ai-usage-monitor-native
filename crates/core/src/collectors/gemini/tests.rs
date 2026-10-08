use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::*;
use crate::collectors::testkit::{cursors_from, DbSink, Fixture, VecSink};
use crate::collectors::{Cursor, ProjectResolver};
use crate::database::testutil::temp_db;

const SID: &str = "f9f78d51-8c47-4d62-8e1c-0c7e0a1d9a11";
const TS: &str = "2026-06-18T06:22:35.160Z";
const TS_MS: i64 = 1_781_763_755_160;

fn header(sid: &str) -> String {
    json!({"sessionId":sid,"projectHash":"abc","startTime":TS,"lastUpdated":TS,"kind":"main"}).to_string()
}

fn gemini_msg(id: &str, input: u64, output: u64, cached: u64, thoughts: u64, tool: u64) -> Value {
    json!({"id":id,"timestamp":TS,"type":"gemini","content":"SECRET PROMPT TEXT must never be stored","thoughts":[{"subject":"x","description":"y"}],
           "tokens":{"input":input,"output":output,"cached":cached,"thoughts":thoughts,"tool":tool,"total":input+output+thoughts+tool},
           "model":"gemini-3-flash-preview"})
}

fn user_msg(id: &str) -> Value {
    json!({"id":id,"timestamp":TS,"type":"user","content":[{"text":"hello"}]})
}

fn set_snapshot(msgs: Vec<Value>) -> String {
    json!({"$set":{"messages":msgs,"lastUpdated":TS}}).to_string()
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
    chats: PathBuf,
}

fn setup() -> Setup {
    let d = tempfile::tempdir().unwrap();
    let chats = d.path().join(".gemini").join("tmp").join("my-project").join("chats");
    std::fs::create_dir_all(&chats).unwrap();
    let fx = Fixture::new(d.path());
    Setup { d, fx, chats }
}

fn run(s: &Setup, cursors: &HashMap<String, Cursor>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
    let det = GeminiCollector.detect(&s.fx.env);
    GeminiCollector.collect_usage(&s.fx.ctx(&det, cursors), sink)
}

fn events_of(lines: &[String]) -> Vec<UsageEvent> {
    let s = setup();
    write_lines(&s.chats.join("session-2026-06-18T06-21-f9f78d51.jsonl"), lines);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    sink.events
}

#[test]
fn splits_cached_out_of_input_and_adds_thoughts_to_output() {
    // input 31491 includes cached 27808; thoughts 48 are additional to output 35.
    let ev = events_of(&[header(SID), gemini_msg("m1", 31_491, 35, 27_808, 48, 0).to_string()]);
    assert_eq!(ev.len(), 1);
    let e = &ev[0];
    assert_eq!((e.input_tokens, e.cache_read_tokens, e.output_tokens, e.reasoning_tokens), (3_683, 27_808, 83, Some(48)));
    assert_eq!(e.total_tokens(), 31_574, "must equal Gemini's own tokens.total (input + output + thoughts + tool)");
    assert_eq!(e.model, "gemini-3-flash-preview");
    assert_eq!(e.ts_utc_ms, TS_MS);
    assert_eq!(e.session_id.as_deref(), Some(SID));
    assert_eq!(e.dedupe_key, format!("gemini:{SID}:m1"));
}

#[test]
fn tool_prompt_tokens_count_as_input() {
    let ev = events_of(&[header(SID), gemini_msg("m1", 1_000, 100, 200, 0, 50).to_string()]);
    assert_eq!(ev[0].input_tokens, 1_000 - 200 + 50);
    assert_eq!(ev[0].total_tokens(), 1_000 + 100 + 50);
}

#[test]
fn snapshot_records_that_repeat_messages_do_not_double_count() {
    // The same message appears directly and inside two $set snapshots (as in real chat files).
    let m = gemini_msg("m1", 1_000, 100, 0, 10, 0);
    let ev = events_of(&[
        header(SID),
        m.to_string(),
        set_snapshot(vec![user_msg("u1"), m.clone()]),
        set_snapshot(vec![user_msg("u1"), m.clone(), gemini_msg("m2", 2_000, 200, 0, 20, 0)]),
    ]);
    assert_eq!(ev.len(), 2, "m1 once, m2 once");
    assert_eq!(ev.iter().map(|e| e.total_tokens()).sum::<u64>(), 1_110 + 2_220);
}

#[test]
fn the_same_message_id_in_different_sessions_is_not_confused() {
    let s = setup();
    write_lines(&s.chats.join("session-a.jsonl"), &[header("sess-a"), gemini_msg("m1", 100, 10, 0, 0, 0).to_string()]);
    write_lines(&s.chats.join("session-b.jsonl"), &[header("sess-b"), gemini_msg("m1", 100, 10, 0, 0, 0).to_string()]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 2);
}

#[test]
fn non_gemini_messages_and_messages_without_tokens_are_ignored_not_counted() {
    let no_tokens = json!({"id":"m9","timestamp":TS,"type":"gemini","content":"in flight"}).to_string();
    let ev = events_of(&[header(SID), user_msg("u1").to_string(), no_tokens]);
    assert!(ev.is_empty());
}

#[test]
fn zero_usage_undated_or_idless_messages_are_skipped_and_counted() {
    let s = setup();
    let undated = json!({"id":"m2","timestamp":"???","type":"gemini","tokens":{"input":5,"output":5}}).to_string();
    let idless = json!({"timestamp":TS,"type":"gemini","tokens":{"input":5,"output":5}}).to_string();
    write_lines(&s.chats.join("session-x.jsonl"), &[header(SID), gemini_msg("m1", 0, 0, 0, 0, 0).to_string(), undated, idless]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert!(sink.events.is_empty());
    assert_eq!(sum.skipped_records, 3);
}

#[test]
fn cached_larger_than_input_is_clamped_instead_of_underflowing() {
    let ev = events_of(&[header(SID), gemini_msg("m1", 100, 10, 500, 0, 0).to_string()]);
    assert_eq!(ev[0].input_tokens, 0);
    assert_eq!(ev[0].cache_read_tokens, 100);
}

#[test]
fn prompt_text_never_reaches_the_events() {
    let ev = events_of(&[header(SID), gemini_msg("m1", 100, 10, 0, 0, 0).to_string()]);
    assert!(!format!("{ev:?}").contains("SECRET PROMPT TEXT"));
}

#[test]
fn legacy_json_chat_files_are_supported() {
    let s = setup();
    let doc = json!({"sessionId": SID, "projectHash": "h", "startTime": TS, "lastUpdated": TS,
                     "messages": [user_msg("u1"), gemini_msg("m1", 1_000, 100, 400, 20, 0)]});
    std::fs::write(s.chats.join("session-legacy.json"), doc.to_string()).unwrap();
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sink.events[0].total_tokens(), 1_120);
    // second pass: unchanged → not re-read
    let sum = run(&s, &cursors_from(&sink.cursors), &mut VecSink::default()).unwrap();
    assert_eq!(sum.files_read, 0);
}

#[test]
fn a_corrupt_legacy_file_is_retried_not_treated_as_zero_usage() {
    let s = setup();
    std::fs::write(s.chats.join("session-bad.json"), "{ not json").unwrap();
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert!(sink.cursors.is_empty(), "no cursor may be saved for a file we could not read");
    assert!(sum.notes.iter().any(|n| n.contains("unreadable")));
}

#[test]
fn the_project_comes_from_project_root() {
    let s = setup();
    let work = s.d.path().join("work").join("fund-stack");
    std::fs::create_dir_all(work.join(".git")).unwrap();
    std::fs::write(s.chats.parent().unwrap().join(".project_root"), work.to_string_lossy().as_bytes()).unwrap();
    write_lines(&s.chats.join("session-p.jsonl"), &[header(SID), gemini_msg("m1", 100, 10, 0, 0, 0).to_string()]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events[0].project.as_ref().unwrap().name, "fund-stack");
}

#[test]
fn tailing_resumes_with_the_saved_session_id_and_reads_only_new_lines() {
    let s = setup();
    let file = s.chats.join("session-t.jsonl");
    write_lines(&file, &[header(SID), gemini_msg("m1", 100, 10, 0, 0, 0).to_string()]);
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    run(&s, &HashMap::new(), &mut sink).unwrap();

    append(&file, &format!("{}\n", gemini_msg("m2", 300, 30, 0, 0, 0)));
    let cursors = db.with_reader(|c| crate::database::queries::load_cursors(c, "gemini")).unwrap();
    run(&s, &cursors, &mut sink).unwrap();
    let totals = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap();
    assert_eq!(totals["gemini"].events, 2);
    assert_eq!(totals["gemini"].total_all(), 110 + 330);
}

#[test]
fn a_line_cut_before_the_tokens_keyword_is_not_lost() {
    let s = setup();
    let file = s.chats.join("session-h.jsonl");
    let full = gemini_msg("m2", 500, 50, 0, 0, 0).to_string();
    let cut = full.find("\"tokens\"").unwrap() - 4;
    write_lines(&file, &[header(SID), gemini_msg("m1", 100, 10, 0, 0, 0).to_string()]);
    append(&file, &full[..cut]);
    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    assert_eq!(first.events.len(), 1);

    append(&file, &format!("{}\n", &full[cut..]));
    let mut second = VecSink::default();
    run(&s, &cursors_from(&first.cursors), &mut second).unwrap();
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0].dedupe_key, format!("gemini:{SID}:m2"));
}

#[test]
fn detection_reports_data_only_when_the_cli_is_not_on_path() {
    let d = tempfile::tempdir().unwrap();
    let env = Env::with_home(d.path());
    assert_eq!(GeminiCollector.detect(&env).presence, Presence::NotFound);
    std::fs::create_dir_all(d.path().join(".gemini").join("tmp")).unwrap();
    let det = GeminiCollector.detect(&env);
    assert_eq!(det.presence, Presence::DataOnly);
    assert!(det.note.unwrap().contains("not on PATH"));
}

#[test]
fn historical_only_data_says_when_it_was_last_active() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join(".gemini").join("tmp")).unwrap();
    let det = GeminiCollector.detect(&Env::with_home(d.path()));
    let h = GeminiCollector.get_health(&det, Some(&crate::collectors::RunOutcome::Ok(CollectSummary::default())), 5, Some(TS_MS));
    assert!(h.note.unwrap().contains("last activity 2026-06-18"));
}

// ---------------------------------------------------------------------------------------------------
// Real-data regression (opt-in): `cargo test gemini::tests::real -- --ignored --nocapture`
// ---------------------------------------------------------------------------------------------------
#[test]
#[ignore = "reads the real ~/.gemini directory"]
fn real_data_matches_an_independent_count() {
    let env = Env::from_system();
    let root = tmp_dir(&env);
    if !root.is_dir() {
        eprintln!("no ~/.gemini/tmp here; nothing to verify");
        return;
    }

    // Reference: untyped parse of every chat file; unique (sessionId, id) → max tokens.
    let mut reference: HashMap<(String, String), [u64; 4]> = HashMap::new(); // fresh, cached, output(+thoughts), total
    let mut raw = 0u64;
    let mut consider = |sid: &str, m: &Value| {
        if m["type"] != "gemini" || !m["tokens"].is_object() {
            return;
        }
        raw += 1;
        let (Some(id), true) = (m["id"].as_str(), DateTime::parse_from_rfc3339(m["timestamp"].as_str().unwrap_or("")).is_ok()) else { return };
        let n = |k: &str| m["tokens"][k].as_u64().unwrap_or(0);
        let (i, o, c, th, tool) = (n("input"), n("output"), n("cached").min(n("input")), n("thoughts"), n("tool"));
        if i + o + th + tool == 0 {
            return;
        }
        let cur = [i - c + tool, c, o + th, i + o + th + tool];
        let e = reference.entry((sid.to_string(), id.to_string())).or_insert([0; 4]);
        for k in 0..4 {
            e[k] = e[k].max(cur[k]);
        }
    };
    for project in walk::subdirs(&root) {
        let chats = project.join("chats");
        for (path, _) in walk::files_with_suffix(&chats, ".jsonl", 0) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let mut sid = String::new();
            for line in text.lines() {
                let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
                if let Some(s) = v["sessionId"].as_str() {
                    if sid.is_empty() {
                        sid = s.to_string();
                    }
                }
                consider(&sid, &v);
                if let Some(arr) = v["$set"]["messages"].as_array() {
                    arr.iter().for_each(|m| consider(&sid, m));
                }
            }
        }
        for (path, _) in walk::files_with_suffix(&chats, ".json", 0) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
            let sid = v["sessionId"].as_str().unwrap_or("").to_string();
            if let Some(arr) = v["messages"].as_array() {
                arr.iter().for_each(|m| consider(&sid, m));
            }
        }
    }

    let (_d, db) = temp_db();
    let det = GeminiCollector.detect(&env);
    let fx = Fixture::new(&env.home);
    let projects = ProjectResolver::new(true, env.home.clone());
    let cursors = HashMap::new();
    let ctx = CollectCtx { env: &env, detection: &det, cursors: &cursors, projects: &projects, cancel: &fx.cancel };
    let mut sink = DbSink { db: db.clone(), agent: ID };
    let sum = GeminiCollector.collect_usage(&ctx, &mut sink).unwrap();

    let (count, _) = db.with_reader(|c| crate::database::queries::agent_event_stats(c, "gemini")).unwrap();
    let t = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap()["gemini"];
    let ref_sum = reference.values().fold([0u64; 4], |mut a, e| {
        for k in 0..4 {
            a[k] += e[k];
        }
        a
    });
    eprintln!("token-bearing records: {raw}; unique messages (reference): {}; collector events: {count}; skipped {}", reference.len(), sum.skipped_records);
    eprintln!("reference [fresh,cached,output,total]: {ref_sum:?}; collector: [{},{},{},{}]", t.input, t.cache_read, t.output, t.total_all());
    assert_eq!(count, reference.len() as u64);
    assert_eq!([t.input, t.cache_read, t.output, t.total_all()], ref_sum);
    assert!(count < raw, "de-duplication must reduce the record count");
}
