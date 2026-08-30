//! The token count that Claude Code does not put in the session file.
//!
//! This is the only inexact join in this tool. The other joins use a pid and its start time,
//! which are exact. This module makes a path from `cwd`, and Claude Code makes that path with a
//! `-` for each `/` and each `.`. Two different directories can give the same path: for example
//! `/home/x/.config` and `/home/x-config`. A hit is therefore almost certainly the transcript of
//! the session, but not certainly. A wrong result costs one key, and the key is `null` if the
//! file is not there.
//!
//! There is no percentage here. Claude Code computes the size of the context window in memory
//! and does not write it to disk. Consumers get the token count.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How many bytes of the end of a transcript to read to find the last assistant turn.
///
/// A transcript has no limit on its size, and a consumer can read all agents every few seconds.
/// A limit keeps the cost of this tool constant. The last assistant turn is at the end of the
/// file, so this window finds it.
const TAIL_BYTES: u64 = 256 * 1024;

/// How loaded a session's context is, in tokens.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// All tokens sent for the most recent assistant turn: new input, cache writes and cache
    /// reads. Output tokens are not included, because they go in the next request.
    pub tokens: u64,
    /// Epoch seconds of that turn.
    ///
    /// The token count is from the last completed assistant turn, so a session that is `busy`
    /// has added tokens after this time. This stamp lets a consumer see how old the count is.
    pub as_of: u64,
}

/// `~/.claude/projects/<slug>/<session_id>.jsonl`, where the slug is `cwd` with every `/` and
/// every `.` replaced by `-`.
fn transcript_path(home: &str, cwd: &str, session_id: &str) -> PathBuf {
    let slug: String = cwd
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(slug)
        .join(format!("{session_id}.jsonl"))
}

/// The context this session was carrying at its last assistant turn.
pub fn context_of(home: &str, cwd: &str, session_id: &str) -> Option<Context> {
    let text = tail(&transcript_path(home, cwd, session_id), TAIL_BYTES)?;
    last_assistant_usage(&text)
}

/// The last `max` bytes of a file, from the start of a line.
///
/// A read that starts in the middle of the file drops its first line, because that line is part
/// of a record. The bytes are decoded lossily, so one bad byte does not cost the full answer.
fn tail(path: &PathBuf, max: u64) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start)).ok()?;

    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();

    if start == 0 {
        return Some(text);
    }
    let first_break = text.find('\n')?;
    Some(text[first_break + 1..].to_string())
}

/// One transcript record, as much of it as this module reads.
#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    /// Claude Code writes subagent turns to the same file. The usage of a subagent turn is the
    /// context of the subagent, and not the context of this session.
    #[serde(rename = "isSidechain", default)]
    is_sidechain: bool,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

/// Find the newest assistant turn that reports usage. The scan goes backwards, because the
/// answer is the last such turn and a transcript has many records before it.
///
/// A record that does not parse is skipped and does not stop the scan. Claude Code appends to
/// this file while this tool reads it, so the last line is frequently incomplete.
pub fn last_assistant_usage(text: &str) -> Option<Context> {
    for line in text.lines().rev() {
        let Ok(entry) = serde_json::from_str::<Entry>(line) else {
            continue;
        };
        if entry.kind.as_deref() != Some("assistant") || entry.is_sidechain {
            continue;
        }
        let Some(usage) = entry.message.and_then(|m| m.usage) else {
            continue;
        };
        return Some(Context {
            tokens: usage.input_tokens
                + usage.cache_creation_input_tokens
                + usage.cache_read_input_tokens,
            as_of: entry
                .timestamp
                .as_deref()
                .and_then(iso_epoch_secs)
                .unwrap_or(0),
        });
    }
    None
}

/// `2026-08-30T01:10:21.036Z` as epoch seconds.
///
/// The format is fixed, the time is always UTC, and only whole seconds are necessary, so this
/// module does not use a date crate. A stamp that does not parse gives `0`.
fn iso_epoch_secs(iso: &str) -> Option<u64> {
    let bytes = iso.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let num = |a: usize, b: usize| iso.get(a..b)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);

    // Days from the civil date, by Howard Hinnant's algorithm: shift the year to start in
    // March so a leap day is the last day of the year and needs no special case.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    let secs = days * 86_400 + hh * 3_600 + mm * 60 + ss;
    u64::try_from(secs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slug_replaces_both_slashes_and_dots() {
        let path = transcript_path("/home/you", "/home/you/.config/nixos", "abc");
        assert!(path.ends_with("projects/-home-you--config-nixos/abc.jsonl"));

        let path = transcript_path("/home/you", "/home/you/Work/infra.git/master", "abc");
        assert!(path.ends_with("projects/-home-you-Work-infra-git-master/abc.jsonl"));
    }

    const ASSISTANT: &str = r#"{"type":"assistant","isSidechain":false,"timestamp":"2026-08-30T01:10:21.036Z","message":{"usage":{"input_tokens":2,"cache_creation_input_tokens":2015,"cache_read_input_tokens":185936,"output_tokens":905}}}"#;

    #[test]
    fn sums_the_three_input_kinds_and_not_the_output() {
        let context = last_assistant_usage(ASSISTANT).unwrap();
        assert_eq!(context.tokens, 2 + 2015 + 185_936);
        assert_eq!(context.as_of, 1_788_052_221);
    }

    #[test]
    fn the_last_assistant_turn_is_the_answer() {
        let older = ASSISTANT.replace("185936", "1000");
        let text = format!("{older}\n{ASSISTANT}\n");
        assert_eq!(last_assistant_usage(&text).unwrap().tokens, 187_953);
    }

    #[test]
    fn a_sidechain_turn_is_not_this_sessions_context() {
        let sidechain = ASSISTANT.replace(r#""isSidechain":false"#, r#""isSidechain":true"#);
        assert_eq!(last_assistant_usage(&sidechain), None);

        // ...and it does not shadow the real answer sitting behind it.
        let text = format!("{ASSISTANT}\n{sidechain}\n");
        assert_eq!(last_assistant_usage(&text).unwrap().tokens, 187_953);
    }

    #[test]
    fn a_half_written_line_is_skipped_not_fatal() {
        let text = format!("{ASSISTANT}\n{{\"type\":\"assis");
        assert_eq!(last_assistant_usage(&text).unwrap().tokens, 187_953);
    }

    #[test]
    fn no_assistant_turn_is_none_not_zero() {
        assert_eq!(last_assistant_usage(r#"{"type":"user"}"#), None);
        assert_eq!(last_assistant_usage(""), None);
    }

    #[test]
    fn iso_stamps_convert_to_epoch_seconds() {
        assert_eq!(iso_epoch_secs("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(
            iso_epoch_secs("2026-08-30T01:10:21.036Z"),
            Some(1_788_052_221)
        );
        // A leap day, which is what the March-shifted year arithmetic exists for.
        assert_eq!(
            iso_epoch_secs("2024-02-29T12:00:00.000Z"),
            Some(1_709_208_000)
        );
        assert_eq!(iso_epoch_secs("nonsense"), None);
    }
}
