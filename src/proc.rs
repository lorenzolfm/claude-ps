//! What the kernel knows about a process, and the two parsers that read it.
//!
//! Both parsers are pure and take bytes, because both have a case that is easy to get wrong
//! and impossible to exercise through the filesystem on demand: a `comm` containing spaces and
//! parentheses, and an environment entry that is not valid UTF-8.

use std::fs;
use std::path::PathBuf;

fn proc_path(pid: u32, leaf: &str) -> PathBuf {
    let mut path = PathBuf::from("/proc");
    path.push(pid.to_string());
    path.push(leaf);
    path
}

/// The process start time, field 22 of `/proc/<pid>/stat`, as the kernel spelled it.
///
/// Returned as a string rather than a number on purpose: it is only ever compared for equality
/// against the value Claude recorded, and parsing it would invite a lossy round-trip into that
/// comparison for no gain.
pub fn start_time(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(proc_path(pid, "stat")).ok()?;
    parse_start_time(&stat).map(str::to_owned)
}

/// Field 22 of a `/proc/<pid>/stat` line.
///
/// 🔴 Parsed after the **last** `)`, never by splitting the whole line. Field 2 is `comm`, the
/// executable name, and it is wrapped in parentheses but *not* escaped — a process called
/// `(sd-pam)` renders as `((sd-pam))`, and one with a space in its name splits the line into a
/// different number of fields. Everything after the final `)` is field 3 onward, so the start
/// time is index 19 of that.
pub fn parse_start_time(stat: &str) -> Option<&str> {
    let close = stat.rfind(')')?;
    stat[close + 1..].split_whitespace().nth(19)
}

/// The agent's own `(ZELLIJ_SESSION_NAME, ZELLIJ_PANE_ID)`, or `None` for either it lacks.
///
/// This is the half of the join Claude does not write down: the session file says what an agent
/// is doing and nothing about where it is, and the answer is only ever in its environment.
pub fn zellij_of(pid: u32) -> (Option<String>, Option<String>) {
    match fs::read(proc_path(pid, "environ")) {
        Ok(raw) => parse_environ(&raw),
        Err(_) => (None, None),
    }
}

/// `/proc/<pid>/environ` is NUL-separated `KEY=VALUE`, and a value is arbitrary bytes.
///
/// Decoded lossily rather than rejected: a session name that is not valid UTF-8 is a session
/// name we should still show, mangled, instead of dropping the agent off the list entirely.
pub fn parse_environ(raw: &[u8]) -> (Option<String>, Option<String>) {
    let mut session = None;
    let mut pane = None;
    for entry in raw.split(|byte| *byte == 0) {
        let Some(eq) = entry.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let (key, value) = entry.split_at(eq);
        let value = || String::from_utf8_lossy(&value[1..]).into_owned();
        match key {
            b"ZELLIJ_SESSION_NAME" => session = Some(value()),
            b"ZELLIJ_PANE_ID" => pane = Some(value()),
            _ => {}
        }
    }
    (session, pane)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fields 3..21 of a real line, so the count between the `)` and the start time is the
    /// kernel's and not one this test invented. Verified against a live process: field 22 here
    /// is the same number Claude records as `procStart`.
    const FIELDS_3_TO_21: &str = "R 1 1 1 0 -1 4194304 328 0 0 0 0 0 0 0 20 0 1 0";

    #[test]
    fn start_time_is_field_22() {
        let stat = format!("3520542 (cat) {FIELDS_3_TO_21} 41288167 17489920 1165");
        assert_eq!(parse_start_time(&stat), Some("41288167"));
    }

    /// 🔴 The case that makes splitting the whole line wrong. `comm` is not escaped, so both a
    /// space and a nested `)` can appear inside field 2 and shift every later index.
    #[test]
    fn start_time_survives_parens_and_spaces_in_comm() {
        let stat = format!("1 ((sd pam)) {FIELDS_3_TO_21} 555 17489920 1165");
        assert_eq!(parse_start_time(&stat), Some("555"));
    }

    #[test]
    fn start_time_rejects_a_truncated_line() {
        assert_eq!(parse_start_time("42 (claude) S 0 0"), None);
        assert_eq!(parse_start_time("no parens here"), None);
    }

    #[test]
    fn environ_finds_both_zellij_vars() {
        let raw = b"PATH=/usr/bin\0ZELLIJ_SESSION_NAME=work\0ZELLIJ_PANE_ID=3\0";
        let (session, pane) = parse_environ(raw);
        assert_eq!(session.as_deref(), Some("work"));
        assert_eq!(pane.as_deref(), Some("3"));
    }

    /// An agent outside zellij. Both halves absent is the normal answer, not a failure.
    #[test]
    fn environ_without_zellij_yields_nothing() {
        assert_eq!(parse_environ(b"PATH=/usr/bin\0HOME=/root\0"), (None, None));
    }

    /// An entry with no `=` must not be mistaken for a key with an empty value.
    #[test]
    fn environ_ignores_malformed_entries() {
        let raw = b"JUSTAKEY\0\0ZELLIJ_PANE_ID=0\0";
        let (session, pane) = parse_environ(raw);
        assert_eq!(session, None);
        assert_eq!(pane.as_deref(), Some("0"));
    }

    /// Mangled, not dropped.
    #[test]
    fn environ_decodes_invalid_utf8_lossily() {
        let raw = b"ZELLIJ_SESSION_NAME=bad\xffname\0";
        let (session, _) = parse_environ(raw);
        assert_eq!(session.as_deref(), Some("bad\u{fffd}name"));
    }

    /// A value may itself contain `=`; only the first one separates.
    #[test]
    fn environ_splits_on_the_first_equals_only() {
        let (session, _) = parse_environ(b"ZELLIJ_SESSION_NAME=a=b=c\0");
        assert_eq!(session.as_deref(), Some("a=b=c"));
    }
}
