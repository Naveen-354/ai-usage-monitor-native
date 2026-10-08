use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use super::decode::fixture::{blob, sample};
use super::decode::Generation;
use super::wire::enc;
use super::*;
use crate::collectors::testkit::{cursors_from, DbSink, Fixture, VecSink};
use crate::collectors::{Cursor, ProjectResolver};
use crate::database::testutil::temp_db;

const CONV: &str = "5d9392fa-6062-4951-b0ed-af8123ccc5ed";

fn gen(id: &str, input: u64, output: u64, cache_read: u64, secs: i64) -> Generation {
    Generation {
        response_id: Some(id.to_string()),
        input,
        output,
        cache_read,
        thinking: output / 2,
        response_output: output - output / 2,
        created_secs: Some(secs),
        step_index: Some(1),
        ..sample()
    }
}

fn write_db(path: &Path, rows: &[(i64, Vec<u8>)], steps: &[(i64, Vec<u8>)]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let c = Connection::open(path).unwrap();
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS gen_metadata (idx INTEGER PRIMARY KEY, data BLOB, size INTEGER);
         CREATE TABLE IF NOT EXISTS steps (idx INTEGER PRIMARY KEY, step_type INTEGER, metadata BLOB);",
    )
    .unwrap();
    for (idx, data) in rows {
        c.execute("INSERT OR REPLACE INTO gen_metadata (idx, data, size) VALUES (?1, ?2, ?3)", params![idx, data, data.len() as i64]).unwrap();
    }
    for (idx, meta) in steps {
        c.execute("INSERT OR REPLACE INTO steps (idx, step_type, metadata) VALUES (?1, 1, ?2)", params![idx, meta]).unwrap();
    }
}

fn write_summaries(dir: &Path, rows: &[(&str, &str)]) {
    let c = Connection::open(dir.join("conversation_summaries.db")).unwrap();
    c.execute_batch("CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, preview TEXT, workspace_uris TEXT);").unwrap();
    for (id, uris) in rows {
        c.execute("INSERT INTO conversation_summaries VALUES (?1, 'SECRET TITLE must never be read', 'SECRET PREVIEW', ?2)", params![id, uris]).unwrap();
    }
}

struct Setup {
    d: tempfile::TempDir,
    fx: Fixture,
    cli: PathBuf,
    db: PathBuf,
}

fn setup() -> Setup {
    let d = tempfile::tempdir().unwrap();
    let cli = d.path().join(".gemini").join("antigravity-cli").join("conversations");
    std::fs::create_dir_all(&cli).unwrap();
    let db = cli.join(format!("{CONV}.db"));
    let fx = Fixture::new(d.path());
    Setup { d, fx, cli, db }
}

fn run(s: &Setup, cursors: &HashMap<String, Cursor>, sink: &mut dyn BatchSink) -> Result<CollectSummary, CollectError> {
    let det = AntigravityCollector.detect(&s.fx.env);
    AntigravityCollector.collect_usage(&s.fx.ctx(&det, cursors), sink)
}

fn row(idx: i64, g: &Generation) -> (i64, Vec<u8>) {
    (idx, blob(g, true))
}

#[test]
fn decodes_usage_into_disjoint_counters_with_the_response_id_as_dedupe_key() {
    let s = setup();
    write_db(&s.db, &[row(0, &gen("resp-1", 2_049, 24, 16_263, 1_782_892_239))], &[]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    let e = &sink.events[0];
    assert_eq!((e.input_tokens, e.output_tokens, e.cache_read_tokens, e.cache_write_tokens), (2_049, 24, 16_263, 0));
    assert_eq!(e.reasoning_tokens, Some(12));
    assert_eq!(e.model, "gemini-pro-default");
    assert_eq!(e.ts_utc_ms, 1_782_892_239_000);
    assert_eq!(e.dedupe_key, "agy:resp-1");
    assert_eq!(e.session_id.as_deref(), Some(CONV));
    assert_eq!(e.accuracy, Accuracy::Real);
}

#[test]
fn rows_without_a_start_time_are_dated_from_their_step() {
    // The newer 70% of real rows: no created_at in the blob. Without this fallback they would vanish.
    let s = setup();
    let mut g = gen("resp-2", 100, 10, 0, 0);
    g.created_secs = None;
    g.step_index = Some(42);
    write_db(&s.db, &[(0, blob(&g, false))], &[(42, enc::msg(1, &[enc::v(1, 1_790_000_000)]))]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sink.events[0].ts_utc_ms, 1_790_000_000_000);
    assert_eq!(sum.skipped_records, 0);
}

#[test]
fn rows_that_cannot_be_dated_are_skipped_and_counted_never_given_a_guessed_time() {
    let s = setup();
    let mut g = gen("resp-3", 100, 10, 0, 0);
    g.created_secs = None;
    g.step_index = Some(99); // no such step
    write_db(&s.db, &[(0, blob(&g, false)), row(1, &gen("resp-4", 50, 5, 0, 1_790_000_000))], &[]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sum.skipped_records, 1);
    assert!(sum.notes.iter().any(|n| n.contains("no recoverable timestamp")));
}

#[test]
fn all_zero_rows_are_skipped_not_stored() {
    let s = setup();
    write_db(&s.db, &[row(0, &gen("z", 0, 0, 0, 1_790_000_000)), row(1, &gen("ok", 5, 2, 0, 1_790_000_000))], &[]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sum.skipped_records, 1);
}

#[test]
fn the_retry_copy_of_usage_does_not_double_count() {
    let s = setup();
    write_db(&s.db, &[row(0, &gen("r", 1_000, 100, 5_000, 1_790_000_000))], &[]);
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    run(&s, &HashMap::new(), &mut sink).unwrap();
    let t = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap()["antigravity"];
    assert_eq!((t.input, t.output, t.cache_read, t.events), (1_000, 100, 5_000, 1));
}

#[test]
fn incremental_runs_pick_up_new_rows_and_the_overlap_never_double_counts() {
    let s = setup();
    write_db(&s.db, &[row(0, &gen("a", 100, 10, 0, 1_790_000_000)), row(1, &gen("b", 200, 20, 0, 1_790_000_100))], &[]);
    let (_d, db) = temp_db();
    let mut sink = DbSink { db: db.clone(), agent: ID };
    run(&s, &HashMap::new(), &mut sink).unwrap();

    write_db(&s.db, &[row(2, &gen("c", 300, 30, 0, 1_790_000_200))], &[]);
    let cursors = db.with_reader(|c| crate::database::queries::load_cursors(c, "antigravity")).unwrap();
    run(&s, &cursors, &mut sink).unwrap();

    let t = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap()["antigravity"];
    assert_eq!(t.events, 3);
    assert_eq!(t.input, 600, "rows a and b were re-read through the overlap window but counted once");
}

#[test]
fn an_unchanged_database_is_not_opened_again() {
    let s = setup();
    write_db(&s.db, &[row(0, &gen("a", 100, 10, 0, 1_790_000_000))], &[]);
    let mut first = VecSink::default();
    run(&s, &HashMap::new(), &mut first).unwrap();
    let mut second = VecSink::default();
    let sum = run(&s, &cursors_from(&first.cursors), &mut second).unwrap();
    assert_eq!((sum.files_seen, sum.files_read), (1, 0));
}

#[test]
fn the_project_comes_from_workspace_uris_percent_decoded() {
    let s = setup();
    let work = s.d.path().join("my projects").join("fund-stack");
    std::fs::create_dir_all(work.join(".git")).unwrap();
    let uri = format!("file:///{}", work.to_string_lossy().replace('\\', "/").replace(' ', "%20"));
    write_summaries(s.cli.parent().unwrap(), &[(CONV, &serde_json::to_string(&[uri]).unwrap())]);
    write_db(&s.db, &[row(0, &gen("a", 100, 10, 0, 1_790_000_000))], &[]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events[0].project.as_ref().unwrap().name, "fund-stack");
}

#[test]
fn conversation_titles_and_previews_are_never_read() {
    let s = setup();
    write_summaries(s.cli.parent().unwrap(), &[(CONV, "[]")]);
    write_db(&s.db, &[row(0, &gen("a", 100, 10, 0, 1_790_000_000))], &[]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert!(!format!("{:?}", sink.events).contains("SECRET"));
}

#[test]
fn both_the_cli_and_the_ide_databases_are_collected() {
    let s = setup();
    let ide = s.d.path().join(".gemini").join("antigravity").join("conversations");
    write_db(&s.db, &[row(0, &gen("cli-1", 100, 10, 0, 1_790_000_000))], &[]);
    write_db(&ide.join("11111111-1111-1111-1111-111111111111.db"), &[row(0, &gen("ide-1", 200, 20, 0, 1_790_000_000))], &[]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 2);
}

#[test]
fn a_changed_format_is_reported_unavailable_never_as_zero_tokens() {
    // Rows exist and are valid protobuf, but none has the documented usage block.
    let s = setup();
    write_db(&s.db, &[(0, enc::msg(1, &[enc::v(3, 1020)])), (1, enc::s(4, "x"))], &[]);
    let mut sink = VecSink::default();
    match run(&s, &HashMap::new(), &mut sink) {
        Err(CollectError::Unavailable(reason)) => assert!(reason.contains("not recognised"), "{reason}"),
        other => panic!("expected Unavailable, got {other:?}"),
    }
    assert!(sink.events.is_empty());
}

#[test]
fn undecodable_rows_alone_also_mean_unavailable() {
    let s = setup();
    write_db(&s.db, &[(0, vec![0xff, 0xff, 0xff])], &[]);
    assert!(matches!(run(&s, &HashMap::new(), &mut VecSink::default()), Err(CollectError::Unavailable(_))));
}

#[test]
fn a_few_bad_rows_among_good_ones_do_not_make_the_agent_unavailable() {
    let s = setup();
    write_db(&s.db, &[(0, vec![0xff, 0xff]), row(1, &gen("ok", 10, 1, 0, 1_790_000_000))], &[]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert_eq!(sum.skipped_records, 1);
}

#[test]
fn a_garbage_file_is_unreadable_not_fatal_and_others_still_load() {
    let s = setup();
    std::fs::write(s.cli.join("garbage.db"), b"this is not sqlite at all").unwrap();
    write_db(&s.db, &[row(0, &gen("a", 100, 10, 0, 1_790_000_000))], &[]);
    let mut sink = VecSink::default();
    let sum = run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
    assert!(sum.notes.iter().any(|n| n.contains("unreadable")));
}

#[test]
fn a_database_without_gen_metadata_is_ignored() {
    let s = setup();
    let c = Connection::open(s.cli.join("other.db")).unwrap();
    c.execute_batch("CREATE TABLE unrelated (x);").unwrap();
    drop(c);
    write_db(&s.db, &[row(0, &gen("a", 100, 10, 0, 1_790_000_000))], &[]);
    let mut sink = VecSink::default();
    run(&s, &HashMap::new(), &mut sink).unwrap();
    assert_eq!(sink.events.len(), 1);
}

#[test]
fn uri_decoding_handles_windows_posix_and_hostile_input() {
    assert_eq!(uri_to_path("file:///D:/invest%20mf/codebase/Inveztmf%20-%20base%201").as_deref(), Some("D:/invest mf/codebase/Inveztmf - base 1"));
    assert_eq!(uri_to_path("file:///d%3A/github/schemaforge").as_deref(), Some("d:/github/schemaforge"));
    assert_eq!(uri_to_path("file:///home/me/proj").as_deref(), Some("/home/me/proj"));
    assert_eq!(uri_to_path("https://example.com"), None);
    assert_eq!(uri_to_path("file://"), None);
    // never panics on odd percent sequences or multibyte characters right after '%'
    for bad in ["file:///a%", "file:///a%2", "file:///a%zz", "file:///a%é", "file:///%ff%fe", "file:///é%é"] {
        let _ = uri_to_path(bad);
    }
}

#[test]
fn detection_distinguishes_installed_data_only_and_absent() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(AntigravityCollector.detect(&Env::with_home(d.path())).presence, Presence::NotFound);
    std::fs::create_dir_all(d.path().join(".gemini").join("antigravity").join("conversations")).unwrap();
    let det = AntigravityCollector.detect(&Env::with_home(d.path()));
    assert_eq!(det.presence, Presence::DataOnly);
    assert_eq!(det.roots.len(), 1);

    let mut env = Env::with_home(d.path());
    let bin = d.path().join("local").join("agy").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(if cfg!(windows) { "agy.exe" } else { "agy" }), b"").unwrap();
    env.local_app_data = Some(d.path().join("local"));
    assert_eq!(AntigravityCollector.detect(&env).presence, Presence::Installed);
}

// ---------------------------------------------------------------------------------------------------
// Real-data regression (opt-in): `cargo test antigravity::tests::real -- --ignored --nocapture`
// Checks the invariants measured on a real install rather than copying numbers.
// ---------------------------------------------------------------------------------------------------
#[test]
#[ignore = "reads the real ~/.gemini/antigravity* directories"]
fn real_data_satisfies_the_documented_invariants() {
    let env = Env::from_system();
    let roots = conversation_dirs(&env);
    if roots.is_empty() {
        eprintln!("no Antigravity data here; nothing to verify");
        return;
    }

    // Independent check straight from the blobs: output == thinking + response, response ids unique.
    let (mut rows, mut decoded, mut violations, mut dated_by_blob, mut cache_gt_input) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut ids = std::collections::HashSet::new();
    let mut dup = 0u64;
    for root in &roots {
        for (path, _) in walk::files_with_suffix(root, ".db", 0) {
            let Ok(c) = open_read_only(&path) else { continue };
            if !has_table(&c, "gen_metadata").unwrap_or(false) {
                continue;
            }
            let mut st = c.prepare("SELECT data FROM gen_metadata").unwrap();
            let it = st.query_map([], |r| r.get::<_, Vec<u8>>(0)).unwrap();
            for b in it.flatten() {
                rows += 1;
                let Ok(Some(g)) = decode::decode_generation(&b) else { continue };
                decoded += 1;
                if g.total() == 0 {
                    continue;
                }
                if g.output != g.thinking + g.response_output {
                    violations += 1;
                }
                if g.cache_read > g.input {
                    cache_gt_input += 1;
                }
                if g.created_secs.is_some() {
                    dated_by_blob += 1;
                }
                if let Some(id) = g.response_id {
                    if !ids.insert(id) {
                        dup += 1;
                    }
                }
            }
        }
    }

    let (_d, db) = temp_db();
    let det = AntigravityCollector.detect(&env);
    let fx = Fixture::new(&env.home);
    let projects = ProjectResolver::new(true, env.home.clone());
    let cursors = HashMap::new();
    let ctx = CollectCtx { env: &env, detection: &det, cursors: &cursors, projects: &projects, cancel: &fx.cancel };
    let mut sink = DbSink { db: db.clone(), agent: ID };
    let sum = AntigravityCollector.collect_usage(&ctx, &mut sink).unwrap();
    let (count, last) = db.with_reader(|c| crate::database::queries::agent_event_stats(c, "antigravity")).unwrap();
    let t = db.with_reader(|c| crate::database::queries::totals_by_agent(c, 0, i64::MAX / 2)).unwrap()["antigravity"];

    eprintln!("rows {rows}; decoded {decoded}; unique response ids {}; duplicates {dup}", ids.len());
    eprintln!("output != thinking+response: {violations}; cache_read > input: {cache_gt_input}; dated by blob: {dated_by_blob} (rest dated via steps)");
    eprintln!("collector: events {count}; input {} output {} cacheRead {}; skipped {}; last event {:?}", t.input, t.output, t.cache_read, sum.skipped_records, last);

    assert_eq!(violations, 0, "output must equal thinking + response on every generation");
    assert_eq!(dup, 0, "response ids must be globally unique (they are the dedupe key)");
    assert!(count as usize <= ids.len(), "never more events than distinct responses");
    assert!(count > 0 && dated_by_blob < decoded, "the step-timestamp fallback must be exercised on real data");
    assert!(count as f64 >= ids.len() as f64 * 0.99 - 30.0, "collector lost generations: {count} of {}", ids.len());
}
