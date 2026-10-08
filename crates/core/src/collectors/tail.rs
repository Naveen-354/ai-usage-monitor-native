//! Incremental JSONL reading: resume from a byte offset, never consume a half-written line, and
//! survive truncation/rotation. Files are opened with shared access (Rust's std does this on Windows),
//! because agents keep their active session files open while they stream.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Lines longer than this are skipped (and counted) rather than loaded into memory.
const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineVerdict {
    /// The line was understood (or deliberately ignored): advance past it.
    Consumed,
    /// Only meaningful for an unterminated final line that does not parse yet: retry next time.
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TailResult {
    pub new_offset: u64,
    pub size: u64,
    pub mtime_ms: i64,
    /// The file was shorter than the saved offset, so reading restarted from byte 0.
    pub was_reset: bool,
    pub lines: u64,
    pub oversized_lines_skipped: u64,
}

pub fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Has the file moved on since the cursor was saved? (cheap `stat`-only check)
pub fn changed_since(meta: &std::fs::Metadata, cursor_size: u64, cursor_mtime_ms: i64) -> bool {
    meta.len() != cursor_size || mtime_ms(meta) != cursor_mtime_ms
}

/// Read complete lines after `start_offset`, calling `f(line, terminated)` for each.
///
/// A final line without a newline is offered with `terminated == false`; if `f` answers
/// [`LineVerdict::Incomplete`] the offset stays before it so it is re-read once complete.
pub fn read_new_lines(
    path: &Path,
    start_offset: u64,
    mut f: impl FnMut(&str, bool) -> LineVerdict,
) -> io::Result<TailResult> {
    let file = File::open(path)?;
    let meta = file.metadata()?;
    let size = meta.len();
    let mut result = TailResult { size, mtime_ms: mtime_ms(&meta), ..TailResult::default() };

    let mut offset = start_offset;
    if offset > size {
        offset = 0;
        result.was_reset = true;
    }
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    reader.seek(SeekFrom::Start(offset))?;

    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    loop {
        buf.clear();
        let n = read_line_capped(&mut reader, &mut buf)?;
        if n.read == 0 {
            break;
        }
        let terminated = n.terminated;
        if n.oversized {
            result.oversized_lines_skipped += 1;
            offset += n.read as u64;
            continue;
        }
        let mut end = buf.len();
        if terminated {
            end -= 1; // '\n'
            if end > 0 && buf[end - 1] == b'\r' {
                end -= 1;
            }
        }
        let text = String::from_utf8_lossy(&buf[..end]);
        let verdict = if text.trim().is_empty() { LineVerdict::Consumed } else { f(&text, terminated) };
        if !terminated && verdict == LineVerdict::Incomplete {
            break;
        }
        offset += n.read as u64;
        result.lines += 1;
    }
    result.new_offset = offset;
    Ok(result)
}

struct LineRead {
    read: usize,
    terminated: bool,
    oversized: bool,
}

/// `read_until(b'\n')` that refuses to buffer absurdly long lines (it still consumes them).
fn read_line_capped<R: BufRead>(r: &mut R, buf: &mut Vec<u8>) -> io::Result<LineRead> {
    let mut total = 0usize;
    let mut oversized = false;
    loop {
        let (consumed, done) = {
            let chunk = r.fill_buf()?;
            if chunk.is_empty() {
                return Ok(LineRead { read: total, terminated: false, oversized });
            }
            match chunk.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    if !oversized {
                        if buf.len() + i + 1 > MAX_LINE_BYTES {
                            oversized = true;
                            buf.clear();
                        } else {
                            buf.extend_from_slice(&chunk[..=i]);
                        }
                    }
                    (i + 1, true)
                }
                None => {
                    if !oversized {
                        if buf.len() + chunk.len() > MAX_LINE_BYTES {
                            oversized = true;
                            buf.clear();
                        } else {
                            buf.extend_from_slice(chunk);
                        }
                    }
                    (chunk.len(), false)
                }
            }
        };
        r.consume(consumed);
        total += consumed;
        if done {
            return Ok(LineRead { read: total, terminated: true, oversized });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(path: &Path, content: &[u8]) {
        std::fs::write(path, content).unwrap();
    }

    fn collect(path: &Path, start: u64) -> (Vec<String>, TailResult) {
        let mut lines = Vec::new();
        let r = read_new_lines(path, start, |l, _| {
            lines.push(l.to_string());
            LineVerdict::Consumed
        })
        .unwrap();
        (lines, r)
    }

    #[test]
    fn reads_all_complete_lines_and_reports_the_end_offset() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        write(&p, b"{\"a\":1}\n{\"a\":2}\n");
        let (lines, r) = collect(&p, 0);
        assert_eq!(lines, vec!["{\"a\":1}", "{\"a\":2}"]);
        assert_eq!(r.new_offset, 16);
        assert_eq!(r.size, 16);
        assert!(!r.was_reset);
    }

    #[test]
    fn resumes_from_the_saved_offset_without_rereading() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        write(&p, b"one\ntwo\n");
        let (_, first) = collect(&p, 0);
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"three\n").unwrap();
        drop(f);
        let (lines, second) = collect(&p, first.new_offset);
        assert_eq!(lines, vec!["three"]);
        assert_eq!(second.new_offset, 14);
    }

    #[test]
    fn handles_crlf() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        write(&p, b"one\r\ntwo\r\n");
        let (lines, _) = collect(&p, 0);
        assert_eq!(lines, vec!["one", "two"]);
    }

    #[test]
    fn an_unparseable_unterminated_last_line_is_left_for_next_time() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        write(&p, b"{\"ok\":1}\n{\"half\":");
        let mut seen = Vec::new();
        let r = read_new_lines(&p, 0, |l, terminated| {
            seen.push((l.to_string(), terminated));
            if terminated || l.ends_with('}') { LineVerdict::Consumed } else { LineVerdict::Incomplete }
        })
        .unwrap();
        assert_eq!(r.new_offset, 9, "offset must stop before the half-written line");
        assert_eq!(seen.last().unwrap(), &("{\"half\":".to_string(), false));

        // The writer finishes the line; the next read picks it up exactly once.
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"2}\n").unwrap();
        drop(f);
        let (lines, r2) = collect(&p, r.new_offset);
        assert_eq!(lines, vec!["{\"half\":2}"]);
        assert_eq!(r2.new_offset, 20);
    }

    #[test]
    fn a_valid_unterminated_last_line_can_be_consumed() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        write(&p, b"{\"a\":1}\n{\"a\":2}");
        let (lines, r) = collect(&p, 0);
        assert_eq!(lines.len(), 2);
        assert_eq!(r.new_offset, 15);
    }

    #[test]
    fn a_shrunk_file_restarts_from_the_beginning() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        write(&p, b"a-long-first-line\nsecond\n");
        let (_, first) = collect(&p, 0);
        write(&p, b"new\n"); // rotated / truncated
        let (lines, r) = collect(&p, first.new_offset);
        assert!(r.was_reset);
        assert_eq!(lines, vec!["new"]);
    }

    #[test]
    fn blank_lines_are_skipped_but_advance_the_offset() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        write(&p, b"a\n\n\nb\n");
        let (lines, r) = collect(&p, 0);
        assert_eq!(lines, vec!["a", "b"]);
        assert_eq!(r.new_offset, 6);
    }

    #[test]
    fn invalid_utf8_does_not_abort_the_read() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        write(&p, b"ok\n\xff\xfe bad\nafter\n");
        let (lines, _) = collect(&p, 0);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2], "after");
    }

    #[test]
    fn missing_file_is_an_error_not_a_panic() {
        let d = tempfile::tempdir().unwrap();
        assert!(read_new_lines(&d.path().join("nope"), 0, |_, _| LineVerdict::Consumed).is_err());
    }

    #[test]
    fn changed_since_compares_size_and_mtime() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        write(&p, b"abc");
        let m = std::fs::metadata(&p).unwrap();
        assert!(!changed_since(&m, 3, mtime_ms(&m)));
        assert!(changed_since(&m, 4, mtime_ms(&m)));
        assert!(changed_since(&m, 3, mtime_ms(&m) - 1));
    }
}
