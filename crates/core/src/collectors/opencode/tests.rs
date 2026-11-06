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
    assert!(h.note.unwrap().contains("to validate the values"), "the validation limit must always be disclosed");
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

// ---------------------------------------------------------------------------------------------------
// OpenCode 2.x: `session_message` (role in the `type` column) and `session_v2`, as found on OpenCode 2.2.5.
// ---------------------------------------------------------------------------------------------------

/// A database laid out like OpenCode 2.x (the columns that matter, as verified on a real 2.2.5 database).
fn setup_v2() -> Setup {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join(".local").join("share").join("opencode");
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("opencode.db");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, seq INTEGER, time_created INTEGER, time_updated INTEGER, data TEXT);
         CREATE TABLE session_v2 (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT, cost REAL, tokens_input INTEGER, tokens_output INTEGER);
         CREATE TABLE session_part (message_id TEXT, session_id TEXT, ordinal INTEGER, type TEXT, data TEXT);",
    )
    .unwrap();
    let fx = Fixture::new(d.path());
    Setup { d, fx, db }
}

fn put_v2(s: &Setup, id: &str, kind: &str, updated: i64, data: serde_json::Value) {
    let c = Connection::open(&s.db).unwrap();
    c.execute(
        "INSERT OR REPLACE INTO session_message (id, session_id, type, seq, time_created, time_updated, data) VALUES (?1, 'ses_1', ?2, 1, ?3, ?4, ?5)",
        params![id, kind, T0, updated, data.to_string()],
    )
    .unwrap();
}

/// An assistant message as OpenCode 2.x writes it: no `role` (the `type` column has it), the model is an object.
fn assistant_v2(input: u64, output: u64, reasoning: u64, read: u64, write: u64) -> serde_json::Value {
    json!({"time":{"created":T0 + 5,"streamed":T0 + 8,"completed":T0 + 9},"agent":"build",
           "model":{"id":"big-pickle","providerID":"opencode","variant":"default"},
           "content":[{"type":"text","text":"SECRET REPLY TEXT must never be stored"}],"finish":"stop","rawFinish":"stop","cost":0,
           "tokens":{"input":input,"output":output,"reasoning":reasoning,"cache":{"read":read,"write":write}}})
}

#[test]
fn a_2x_assistant_message_is_read_with_its_model_object_and_the_role_column() {
    let s = setup_v2();
    put_v2(&s, "msg_1", "assistant", T0 + 10, assistant_v2(3_849, 12, 0, 3_968, 0));
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    let e = &sink.events[0];
    assert_eq!((e.input_tokens, e.output_tokens, e.cache_read_tokens, e.cache_write_tokens), (3_849, 12, 3_968, 0));
    assert_eq!(e.reasoning_tokens, Some(0));
    assert_eq!(e.model, "big-pickle", "the model name comes from `model.id`");
    assert_eq!(e.ts_utc_ms, T0 + 5);
    assert_eq!(e.dedupe_key, "opencode:msg_1");
    assert_eq!(e.session_id.as_deref(), Some("ses_1"));
    assert!(!format!("{:?}", sink.events).contains("SECRET"), "message text must never reach an event");
}

#[test]
fn a_2x_database_reports_ok_not_unavailable_so_the_agent_shows_its_numbers() {
    let s = setup_v2();
    put_v2(&s, "msg_1", "assistant", T0 + 10, assistant_v2(100, 10, 0, 0, 0));
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    let det = OpenCodeCollector.detect(&s.fx.env);
    let (n, last) = db.with_reader(|c| crate::database::queries::agent_event_stats(c, "opencode")).unwrap();
    let h = OpenCodeCollector.get_health(&det, Some(&crate::collectors::RunOutcome::Ok(sum)), n, last);
    assert_eq!(h.availability, crate::model::Availability::Ok);
    assert_eq!(h.events_total, 1);
}

#[test]
fn user_and_idle_messages_of_a_2x_database_are_ignored() {
    let s = setup_v2();
    put_v2(&s, "u", "user", T0 + 1, json!({"time":{"created":T0 + 1},"text":"SECRET PROMPT TEXT","files":[],"agents":[]}));
    put_v2(&s, "i", "idle", T0 + 2, json!({"time":{"created":T0 + 2},"outcome":"completed"}));
    put_v2(&s, "a0", "assistant", T0 + 3, assistant_v2(0, 0, 0, 0, 0)); // streaming at zero
    put_v2(&s, "a1", "assistant", T0 + 4, assistant_v2(10, 5, 2, 0, 0));
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sink.events[0].output_tokens, 7, "reasoning is counted as output here too");
    assert!(!format!("{:?}", sink.events).contains("SECRET"));
}

#[test]
fn the_2x_project_comes_from_the_session_directory() {
    let s = setup_v2();
    let work = s.d.path().join("work").join("agent-monitor");
    std::fs::create_dir_all(work.join(".git")).unwrap();
    let c = Connection::open(&s.db).unwrap();
    c.execute("INSERT INTO session_v2 (id, directory) VALUES ('ses_1', ?1)", [work.to_string_lossy().as_ref()]).unwrap();
    drop(c);
    put_v2(&s, "m", "assistant", T0 + 1, assistant_v2(10, 5, 0, 0, 0));
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events[0].project.as_ref().unwrap().name, "agent-monitor");
}

#[test]
fn a_2x_row_updated_in_place_is_raised_never_double_counted_and_new_rows_arrive_incrementally() {
    let s = setup_v2();
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    put_v2(&s, "m", "assistant", T0 + 10, assistant_v2(100, 5, 0, 0, 0)); // partial
    run(&s, &HashMap::new(), &mut sink).unwrap();

    put_v2(&s, "m", "assistant", T0 + 20, assistant_v2(100, 400, 0, 0, 0)); // final, same row, later time_updated
    let cursors = db.with_reader(|c| crate::database::queries::load_cursors(c, "opencode")).unwrap();
    run(&s, &cursors, &mut sink).unwrap();
    put_v2(&s, "m2", "assistant", T0 + 5_000_000, assistant_v2(20, 5, 0, 0, 0));
    let cursors = db.with_reader(|c| crate::database::queries::load_cursors(c, "opencode")).unwrap();
    run(&s, &cursors, &mut sink).unwrap();

    let t = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap()["opencode"];
    assert_eq!((t.events, t.input, t.output), (2, 120, 405));
}

#[test]
fn a_2x_assistant_message_without_tokens_means_the_format_changed() {
    let s = setup_v2();
    put_v2(&s, "m", "assistant", T0 + 1, json!({"time":{"created":T0},"model":{"id":"x"},"usage":{"in":1}}));
    match run(&s, &HashMap::new(), &mut VecSink::default()) {
        Err(CollectError::Unavailable(r)) => assert!(r.contains("tokens"), "{r}"),
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn a_database_with_both_layouts_counts_a_message_once_and_reads_both() {
    // A migrated database can hold the same message in `message` and in `session_message`.
    let s = setup();
    Connection::open(&s.db)
        .unwrap()
        .execute_batch(
            "CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT, type TEXT, seq INTEGER, time_created INTEGER, time_updated INTEGER, data TEXT);",
        )
        .unwrap();
    put(&s, "same", T0 + 10, assistant(100, 10, 0, 0, 0));
    put_v2(&s, "same", "assistant", T0 + 10, assistant_v2(100, 10, 0, 0, 0));
    put_v2(&s, "only_v2", "assistant", T0 + 11, assistant_v2(7, 3, 0, 0, 0));
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    run(&s, &HashMap::new(), &mut sink).unwrap();
    let t = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap()["opencode"];
    assert_eq!((t.events, t.input, t.output), (2, 107, 13), "`same` counts once, `only_v2` is read as well");
}

#[test]
fn a_cursor_written_before_2x_was_supported_still_loads_and_reads_the_new_table_from_the_start() {
    let old: State = serde_json::from_str(r#"{"max_updated":123}"#).unwrap();
    assert_eq!((old.max_updated, old.max_updated_v2), (123, None));
    let new = serde_json::to_string(&State { max_updated: 5, max_updated_v2: Some(9) }).unwrap();
    let back: State = serde_json::from_str(&new).unwrap();
    assert_eq!((back.max_updated, back.max_updated_v2), (5, Some(9)));
}

#[test]
fn neither_layout_present_names_both_tables_in_the_reason() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join(".local").join("share").join("opencode");
    std::fs::create_dir_all(&dir).unwrap();
    Connection::open(dir.join("opencode.db")).unwrap().execute_batch("CREATE TABLE other (x);").unwrap();
    let fx = Fixture::new(d.path());
    let det = OpenCodeCollector.detect(&fx.env);
    let cursors = HashMap::new();
    match OpenCodeCollector.collect_usage(&fx.ctx(&det, &cursors), &mut VecSink::default()) {
        Err(CollectError::Unavailable(r)) => assert!(r.contains("session_message") && r.contains("`message`"), "{r}"),
        other => panic!("expected Unavailable, got {other:?}"),
    }
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

// ---------------------------------------------------------------------------------------------------
// Real-data regression (opt-in): `cargo test opencode::tests::real -- --ignored --nocapture`
// Re-derives the answer with a *different* reader (untyped serde_json::Value, either layout) and compares.
// ---------------------------------------------------------------------------------------------------
#[test]
#[ignore = "reads the real ~/.local/share/opencode directory"]
fn real_data_matches_an_independent_count() {
    let env = Env::from_system();
    let path = db_path(&env);
    if !path.is_file() {
        eprintln!("no opencode.db here; nothing to verify");
        return;
    }

    // Independent reference: every assistant message of either layout, de-duplicated by id, per-field maximum.
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).unwrap();
    conn.busy_timeout(std::time::Duration::from_millis(1500)).unwrap();
    let mut reference: HashMap<String, [u64; 4]> = HashMap::new();
    let mut layouts = Vec::new();
    for (table, role_column) in [("message", "NULL"), ("session_message", "type")] {
        if !has_table(&conn, table).unwrap() {
            continue;
        }
        layouts.push(table);
        let mut st = conn.prepare(&format!("SELECT id, data, {role_column} FROM {table}")).unwrap();
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))).unwrap();
        for (id, data, kind) in rows.flatten() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            let role = kind.or_else(|| v["role"].as_str().map(str::to_string));
            if role.as_deref() != Some("assistant") || !v["tokens"].is_object() {
                continue;
            }
            let t = &v["tokens"];
            let n = |x: &serde_json::Value| x.as_u64().unwrap_or(0);
            // OpenCode's own stats count reasoning as output
            let cur = [n(&t["input"]), n(&t["output"]) + n(&t["reasoning"]), n(&t["cache"]["read"]), n(&t["cache"]["write"])];
            if cur.iter().sum::<u64>() == 0 {
                continue;
            }
            let e = reference.entry(id).or_insert([0; 4]);
            for i in 0..4 {
                e[i] = e[i].max(cur[i]);
            }
        }
    }
    drop(conn);

    // The collector, into a real (temp) database, and the health the Diagnostics page would show.
    let (_d, db) = temp_db();
    let det = OpenCodeCollector.detect(&env);
    let fx = Fixture::new(&env.home);
    let projects = crate::collectors::ProjectResolver::new(true, env.home.clone());
    let cursors = HashMap::new();
    let ctx = CollectCtx { env: &env, detection: &det, cursors: &cursors, projects: &projects, cancel: &fx.cancel };
    let mut sink = DbSink { db: db.clone(), agent: ID };
    let outcome = OpenCodeCollector.collect_usage(&ctx, &mut sink);
    let (count, last) = db.with_reader(|c| crate::database::queries::agent_event_stats(c, "opencode")).unwrap();
    let health = match outcome {
        Ok(sum) => OpenCodeCollector.get_health(&det, Some(&crate::collectors::RunOutcome::Ok(sum)), count, last),
        Err(CollectError::Unavailable(r)) => OpenCodeCollector.get_health(&det, Some(&crate::collectors::RunOutcome::Unavailable(r)), count, last),
        Err(e) => panic!("collector failed: {e}"),
    };
    let t = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap().get("opencode").copied().unwrap_or_default();
    let ref_sum: [u64; 4] = reference.values().fold([0; 4], |mut a, e| {
        for i in 0..4 {
            a[i] += e[i];
        }
        a
    });
    eprintln!("layouts present: {layouts:?}; messages with tokens (reference): {}; collector events: {count}", reference.len());
    eprintln!("reference sums [in,out,cacheR,cacheW]: {ref_sum:?}; collector: [{},{},{},{}]", t.input, t.output, t.cache_read, t.cache_write);
    eprintln!("Diagnostics would show: {:?} ({} events)", health.availability, health.events_total);

    assert_eq!(count as usize, reference.len(), "every assistant message with tokens is one event");
    assert_eq!([t.input, t.output, t.cache_read, t.cache_write], ref_sum);
    if !reference.is_empty() {
        assert_eq!(health.availability, crate::model::Availability::Ok, "an OpenCode with usage must not be shown as unavailable");
    }
}
