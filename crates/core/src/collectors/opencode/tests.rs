use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::{params, Connection};
use serde_json::json;

use super::*;
use crate::collectors::testkit::{cursors_from, DbSink, Fixture, VecSink};
use crate::collectors::Cursor;
use crate::database::testutil::temp_db;

const T0: i64 = 1_790_000_000_000;

struct Setup {
    d: tempfile::TempDir,
    fx: Fixture,
    db: PathBuf,
}

/// The tables and columns verified on the real (empty) opencode.db.
fn setup() -> Setup {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join(".local").join("share").join("opencode");
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("opencode.db");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
         CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT, tokens_input INTEGER, tokens_output INTEGER);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, data TEXT);",
    )
    .unwrap();
    let fx = Fixture::new(d.path());
    Setup { d, fx, db }
}

fn put(s: &Setup, id: &str, updated: i64, data: serde_json::Value) {
    let c = Connection::open(&s.db).unwrap();
    c.execute(
        "INSERT OR REPLACE INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, 'ses_1', ?2, ?3, ?4)",
        params![id, T0, updated, data.to_string()],
    )
    .unwrap();
}

fn assistant(input: u64, output: u64, reasoning: u64, read: u64, write: u64) -> serde_json::Value {
    json!({"role":"assistant","modelID":"claude-sonnet-4","providerID":"anthropic","cost":0.12,
           "tokens":{"input":input,"output":output,"reasoning":reasoning,"cache":{"read":read,"write":write}},
           "time":{"created":T0 + 5}, "summary":"SECRET PROMPT TEXT must never be stored"})
}

fn run(s: &Setup, cursors: &HashMap<String, Cursor>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
    let det = OpenCodeCollector.detect(&s.fx.env);
    OpenCodeCollector.collect_usage(&s.fx.ctx(&det, cursors), sink)
}

#[test]
fn maps_tokens_with_reasoning_counted_as_output_like_opencodes_own_stats() {
    let s = setup();
    put(&s, "msg_1", T0 + 10, assistant(1_000, 200, 50, 4_000, 300));
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    let e = &sink.events[0];
    assert_eq!((e.input_tokens, e.output_tokens, e.cache_read_tokens, e.cache_write_tokens), (1_000, 250, 4_000, 300));
    assert_eq!(e.reasoning_tokens, Some(50));
    assert_eq!(e.model, "claude-sonnet-4");
    assert_eq!(e.ts_utc_ms, T0 + 5);
    assert_eq!(e.dedupe_key, "opencode:msg_1");
    assert_eq!(e.session_id.as_deref(), Some("ses_1"));
    assert_eq!(e.accuracy, Accuracy::Real);
}

#[test]
fn an_empty_database_is_ok_with_no_events_not_an_error_and_not_zero_claimed_as_data() {
    let s = setup();
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert!(sink.events.is_empty());
    let det = OpenCodeCollector.detect(&s.fx.env);
    let h = OpenCodeCollector.get_health(&det, Some(&crate::collectors::RunOutcome::Ok(sum)), 0, None);
    assert_eq!(h.availability, crate::model::Availability::NoDataYet);
    assert!(h.note.unwrap().contains("no local usage exists to validate"), "the validation limit must always be disclosed");
}

#[test]
fn user_messages_and_zero_streaming_rows_are_ignored() {
    let s = setup();
    put(&s, "u1", T0 + 1, json!({"role":"user","summary":"SECRET PROMPT TEXT"}));
    put(&s, "a0", T0 + 2, assistant(0, 0, 0, 0, 0)); // just created, streaming at zero
    put(&s, "a1", T0 + 3, assistant(10, 5, 0, 0, 0));
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert!(!format!("{:?}", sink.events).contains("SECRET"));
}

#[test]
fn a_row_updated_in_place_while_streaming_is_raised_never_double_counted() {
    let s = setup();
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    put(&s, "m", T0 + 10, assistant(100, 5, 0, 0, 0)); // partial
    run(&s, &HashMap::new(), &mut sink).unwrap();

    put(&s, "m", T0 + 20, assistant(100, 400, 0, 0, 0)); // final, same row, later time_updated
    let cursors = db.with_reader(|c| crate::database::queries::load_cursors(c, "opencode")).unwrap();
    run(&s, &cursors, &mut sink).unwrap();

    let t = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap()["opencode"];
    assert_eq!((t.events, t.input, t.output), (1, 100, 400));
}

#[test]
fn the_project_comes_from_the_message_cwd_or_the_session_directory() {
    let s = setup();
    let work = s.d.path().join("work").join("agent-monitor");
    std::fs::create_dir_all(work.join(".git")).unwrap();
    let c = Connection::open(&s.db).unwrap();
    c.execute("INSERT INTO session (id, directory) VALUES ('ses_1', ?1)", [work.to_string_lossy().as_ref()]).unwrap();
    drop(c);
    put(&s, "m", T0 + 1, assistant(10, 5, 0, 0, 0)); // no path in the message: falls back to session.directory
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events[0].project.as_ref().unwrap().name, "agent-monitor");
}

#[test]
fn an_unchanged_database_is_not_read_again_and_new_rows_arrive_incrementally() {
    let s = setup();
    put(&s, "m1", T0 + 1, assistant(10, 5, 0, 0, 0));
    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    let mut idle = VecSink::default();
    let sum = run(&s, &cursors_from(&first.cursors), &mut idle).unwrap();
    assert_eq!(sum.files_read, 0);

    put(&s, "m2", T0 + 5_000_000, assistant(20, 5, 0, 0, 0));
    let mut next = VecSink::default();
    run(&s, &cursors_from(&first.cursors), &mut next).unwrap();
    assert!(next.events.iter().any(|e| e.dedupe_key == "opencode:m2"));
}

#[test]
fn assistant_messages_without_tokens_mean_the_format_changed_unavailable_not_zero() {
    let s = setup();
    put(&s, "m", T0 + 1, json!({"role":"assistant","modelID":"x","usage":{"in":1}}));
    match run(&s, &HashMap::new(), &mut VecSink::default()) {
        Err(CollectError::Unavailable(r)) => assert!(r.contains("tokens"), "{r}"),
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn a_missing_message_table_means_the_schema_changed() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join(".local").join("share").join("opencode");
    std::fs::create_dir_all(&dir).unwrap();
    Connection::open(dir.join("opencode.db")).unwrap().execute_batch("CREATE TABLE other (x);").unwrap();
    let fx = Fixture::new(d.path());
    let det = OpenCodeCollector.detect(&fx.env);
    let cursors = HashMap::new();
    assert!(matches!(OpenCodeCollector.collect_usage(&fx.ctx(&det, &cursors), &mut VecSink::default()), Err(CollectError::Unavailable(_))));
}

#[test]
fn an_unparseable_data_column_is_skipped_and_counted() {
    let s = setup();
    let c = Connection::open(&s.db).unwrap();
    c.execute("INSERT INTO message VALUES ('bad', 'ses_1', ?1, ?1, 'not json')", [T0]).unwrap();
    drop(c);
    put(&s, "ok", T0 + 1, assistant(10, 5, 0, 0, 0));
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sum.skipped_records, 1);
}

#[test]
fn detection_honours_xdg_data_home_and_reports_data_only_without_the_binary() {
    let d = tempfile::tempdir().unwrap();
    let custom = d.path().join("xdg");
    std::fs::create_dir_all(custom.join("opencode")).unwrap();
    std::fs::write(custom.join("opencode").join("opencode.db"), b"").unwrap();
    let mut env = Env::with_home(d.path());
    assert_eq!(OpenCodeCollector.detect(&env).presence, Presence::NotFound);
    env.xdg_data_home = Some(custom);
    assert_eq!(OpenCodeCollector.detect(&env).presence, Presence::DataOnly);
}
