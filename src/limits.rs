//! Byte limits for protocol frames and persisted session history.
//!
//! A line limit is not cosmetic: `BufRead::lines()` and `read_until` allocate
//! the whole line before the caller can reject it.  These limits are checked
//! while filling the buffer, so an oversized frame cannot consume memory first
//! and fail later.

use std::io::{self, BufRead};

/// Pi RPC frames may contain tool output and image data.  The limit is high
/// enough for legitimate single-attachment results, but bounds one allocation.
pub const MAX_RPC_FRAME_BYTES: usize = 32 * 1024 * 1024;
/// stderr is diagnostic only; a larger line is dropped and summarized.
pub const MAX_RPC_STDERR_BYTES: usize = 64 * 1024;
/// One prompt may carry several attachments.  The writer rejects anything
/// larger before it reaches the unbounded-part allocation in serde_json.
pub const MAX_RPC_COMMAND_BYTES: usize = 64 * 1024 * 1024;
/// A session line is read one at a time; oversized history lines are skipped.
pub const MAX_SESSION_LINE_BYTES: usize = 32 * 1024 * 1024;
/// Content search keeps at most this much from one persisted message line.
pub const MAX_SEARCH_LINE_BYTES: usize = 32 * 1024 * 1024;
/// Provider-login messages are small; bound both directions independently.
pub const MAX_LOGIN_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_LOGIN_COMMAND_BYTES: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum BoundedLine {
    Line(Vec<u8>),
    TooLong { bytes: usize },
    Eof,
}

/// Reads one LF-terminated line without ever holding more than `limit` bytes
/// (plus the fixed `fill_buf` chunk).  When `drain` is true, the rest of an
/// oversized line is consumed so the next call sees the next line; callers
/// that must stop after a protocol violation pass false.
pub fn read_line_limited<R: BufRead>(
    reader: &mut R,
    limit: usize,
    drain: bool,
) -> io::Result<BoundedLine> {
    let mut line = Vec::new();
    loop {
        let (take, newline, overflow) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return if line.is_empty() {
                    Ok(BoundedLine::Eof)
                } else {
                    Ok(BoundedLine::Line(line))
                };
            }
            let take = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |position| position + 1);
            let newline = available.get(take.saturating_sub(1)) == Some(&b'\n');
            let overflow = line.len().saturating_add(take) > limit;
            if !overflow {
                line.extend_from_slice(&available[..take]);
            }
            (take, newline, overflow)
        };
        reader.consume(take);

        if overflow {
            let mut bytes = line.len().saturating_add(take);
            if drain && !newline {
                loop {
                    let available = reader.fill_buf()?;
                    if available.is_empty() {
                        break;
                    }
                    let take = available
                        .iter()
                        .position(|byte| *byte == b'\n')
                        .map_or(available.len(), |position| position + 1);
                    let done = available.get(take.saturating_sub(1)) == Some(&b'\n');
                    bytes = bytes.saturating_add(take);
                    reader.consume(take);
                    if done {
                        break;
                    }
                }
            }
            return Ok(BoundedLine::TooLong { bytes });
        }
        if newline {
            return Ok(BoundedLine::Line(line));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_lines_under_the_limit_and_reports_eof() {
        let mut input = Cursor::new(b"one\ntwo\n".to_vec());
        assert_eq!(
            read_line_limited(&mut input, 8, false).unwrap(),
            BoundedLine::Line(b"one\n".to_vec())
        );
        assert_eq!(
            read_line_limited(&mut input, 8, false).unwrap(),
            BoundedLine::Line(b"two\n".to_vec())
        );
        assert_eq!(
            read_line_limited(&mut input, 8, false).unwrap(),
            BoundedLine::Eof
        );
    }

    #[test]
    fn an_oversized_line_never_returns_its_contents() {
        let mut input = Cursor::new(b"0123456789\nnext\n".to_vec());
        assert_eq!(
            read_line_limited(&mut input, 4, false).unwrap(),
            BoundedLine::TooLong { bytes: 11 }
        );
        assert_eq!(
            read_line_limited(&mut input, 8, false).unwrap(),
            BoundedLine::Line(b"next\n".to_vec())
        );
    }

    #[test]
    fn draining_skips_the_whole_oversized_line() {
        let mut input = Cursor::new(b"0123456789\nnext\n".to_vec());
        assert_eq!(
            read_line_limited(&mut input, 4, true).unwrap(),
            BoundedLine::TooLong { bytes: 11 }
        );
        assert_eq!(
            read_line_limited(&mut input, 8, true).unwrap(),
            BoundedLine::Line(b"next\n".to_vec())
        );
    }

    #[test]
    fn limits_count_bytes_not_characters() {
        let mut input = Cursor::new("ação\n".as_bytes().to_vec());
        assert_eq!(
            read_line_limited(&mut input, 6, false).unwrap(),
            BoundedLine::TooLong { bytes: 7 }
        );
        let mut input = Cursor::new("ação\n".as_bytes().to_vec());
        assert_eq!(
            read_line_limited(&mut input, 7, false).unwrap(),
            BoundedLine::Line("ação\n".as_bytes().to_vec())
        );
    }
}
