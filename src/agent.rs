//! The session file Claude Code writes, and the row this tool prints for it.

use serde::Deserialize;
use serde_json::Value;

use crate::proc;

/// Placeholder for any field that has no value — including both join columns when the agent is
/// not in zellij. Never an empty field: a consumer splitting on tabs cannot tell an empty field
/// from a missing one, and `-` is visible in a terminal.
pub const NONE: &str = "-";

/// `~/.claude/sessions/<pid>.json`, as much of it as this tool reads.
///
/// Every field is optional and unknown fields are ignored, which is deliberate rather than
/// defensive: this schema belongs to Claude Code and moves with its releases. A field that
/// disappears must cost one column, not the whole row.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub pid: Option<u32>,
    /// Compared against `/proc/<pid>/stat` for liveness. Kept untyped because it has been seen
    /// as a JSON number and there is no reason it could not be written as a string.
    pub proc_start: Option<Value>,
    /// 🔴 Passed through **verbatim**, never matched against a known set. The vocabulary is open
    /// and moves with Claude's version — `shell` appeared in a release after `waiting`, `idle`
    /// and `busy`. A tool that filtered to the statuses it knew would silently drop live agents
    /// every time Claude invented one.
    pub status: Option<String>,
    /// Claude's own derived name, e.g. `zellij-f8`. 🔴 **Not** the zellij session name — it is
    /// the cwd's basename plus a suffix. Emitted because it is cheap, not because it identifies
    /// anything a user typed.
    pub name: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    /// Epoch milliseconds. The first of these three that is present dates the status.
    pub status_updated_at: Option<f64>,
    pub updated_at: Option<f64>,
    pub started_at: Option<f64>,
}

impl SessionFile {
    /// Whether the pid in this file is still the process that wrote it.
    ///
    /// 🔴 Both halves are required. "Is the pid alive" alone is not enough: pids are recycled,
    /// and a stale file whose pid now belongs to something unrelated would hand a consumer that
    /// process's zellij pane. Comparing the start time as well makes the check exact, so
    /// `Enter` in a picker cannot land on a pane that has nothing to do with Claude.
    fn is_live(&self, pid: u32) -> bool {
        let Some(recorded) = self.proc_start.as_ref().map(json_scalar) else {
            return false;
        };
        proc::start_time(pid).is_some_and(|actual| actual == recorded)
    }

    /// Milliseconds since the epoch that the current status was set, if it is known at all.
    fn status_set_at(&self) -> Option<f64> {
        self.status_updated_at
            .or(self.updated_at)
            .or(self.started_at)
    }

    /// The row for this file, or `None` if the agent behind it is gone.
    pub fn row(&self, now_secs: f64) -> Option<Row> {
        let pid = self.pid?;
        if !self.is_live(pid) {
            return None;
        }
        let (session, pane) = proc::zellij_of(pid);
        Some(Row {
            status: self.status.clone().unwrap_or_else(|| NONE.into()),
            age: age_secs(now_secs, self.status_set_at()),
            session: session.unwrap_or_else(|| NONE.into()),
            pane: pane.unwrap_or_else(|| NONE.into()),
            name: self.name.clone().unwrap_or_else(|| NONE.into()),
            pid,
            session_id: self.session_id.clone().unwrap_or_else(|| NONE.into()),
            cwd: self.cwd.clone().unwrap_or_else(|| NONE.into()),
        })
    }
}

/// A JSON scalar as the shortest string that round-trips it, so a `procStart` written as the
/// number `987654` and one written as `"987654"` compare equal.
fn json_scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Whole seconds spent in the current status.
///
/// ⚠️ An unknown timestamp is **zero**, not "now minus the epoch". Reading a missing field as
/// `0` epoch-milliseconds is the obvious shortcut and it renders every row as roughly
/// fifty-seven years old, which looks like data rather than like breakage — so if Claude ever
/// renames these fields the failure is a column of `0s`, which is visibly wrong.
pub fn age_secs(now_secs: f64, status_set_at_ms: Option<f64>) -> u64 {
    let Some(set_at_ms) = status_set_at_ms else {
        return 0;
    };
    let elapsed = now_secs - set_at_ms / 1000.0;
    if elapsed.is_nan() || elapsed <= 0.0 {
        0
    } else {
        elapsed as u64
    }
}

/// One agent, as one output line.
pub struct Row {
    pub status: String,
    pub age: u64,
    pub session: String,
    pub pane: String,
    pub name: String,
    pub pid: u32,
    pub session_id: String,
    pub cwd: String,
}

impl Row {
    /// 🔴 The column order is a published contract — a zellij plugin splits this into exactly
    /// eight fields and reads them by position. `cwd` is last because it is the only field that
    /// can plausibly contain whitespace, so a consumer can take it as the whole remainder of
    /// the line instead of as a field with a terminator.
    pub fn to_tsv(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.status,
            self.age,
            self.session,
            self.pane,
            self.name,
            self.pid,
            self.session_id,
            self.cwd
        )
    }

    /// Deterministic, **not** presentational. Two runs a second apart diff cleanly; deciding
    /// what order a human should see them in belongs to whatever is doing the showing.
    pub fn sort_key(&self) -> (&str, &str, u32) {
        (&self.session, &self.pane, self.pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> Row {
        Row {
            status: "waiting".into(),
            age: 35,
            session: "work".into(),
            pane: "1".into(),
            name: "work-f8".into(),
            pid: 4242,
            session_id: "abc-123".into(),
            cwd: "/home/you/src".into(),
        }
    }

    #[test]
    fn tsv_has_eight_fields_in_the_published_order() {
        let line = row().to_tsv();
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 8);
        assert_eq!(
            fields,
            [
                "waiting",
                "35",
                "work",
                "1",
                "work-f8",
                "4242",
                "abc-123",
                "/home/you/src"
            ]
        );
    }

    /// The reason cwd is last: it is the only field allowed to contain whitespace, and a
    /// consumer must still see exactly eight fields.
    #[test]
    fn a_cwd_with_whitespace_stays_one_trailing_field() {
        let mut row = row();
        row.cwd = "/home/you/my projects/thing".into();
        let line = row.to_tsv();
        assert_eq!(line.splitn(8, '\t').count(), 8);
        assert!(line.ends_with("\t/home/you/my projects/thing"));
    }

    #[test]
    fn age_is_whole_seconds_since_the_status_was_set() {
        assert_eq!(age_secs(1_000.0, Some(940_500.0)), 59);
    }

    /// Clocks disagree; a status stamped in the future is zero, never a huge wrapped number.
    #[test]
    fn age_clamps_a_future_timestamp_to_zero() {
        assert_eq!(age_secs(1_000.0, Some(9_999_000.0)), 0);
    }

    /// ⚠️ The regression this tool's predecessor carried: no timestamp meant an age of ~57
    /// years, which reads as data rather than as breakage.
    #[test]
    fn age_of_an_undated_status_is_zero_not_the_epoch() {
        assert_eq!(age_secs(1_755_000_000.0, None), 0);
    }

    #[test]
    fn proc_start_compares_equal_as_number_or_string() {
        assert_eq!(json_scalar(&serde_json::json!(987654)), "987654");
        assert_eq!(json_scalar(&serde_json::json!("987654")), "987654");
    }

    /// Unknown fields are ignored and missing ones are `None`, so a schema change costs a
    /// column rather than the row.
    #[test]
    fn session_file_tolerates_a_moving_schema() {
        let file: SessionFile =
            serde_json::from_str(r#"{"pid":7,"status":"shell","somethingNew":true,"cwd":"/tmp"}"#)
                .expect("unknown fields must not be an error");
        assert_eq!(file.pid, Some(7));
        assert_eq!(file.status.as_deref(), Some("shell"));
        assert_eq!(file.name, None);
        assert_eq!(file.status_set_at(), None);
    }

    #[test]
    fn status_set_at_prefers_the_most_specific_stamp() {
        let file: SessionFile =
            serde_json::from_str(r#"{"statusUpdatedAt":3,"updatedAt":2,"startedAt":1}"#).unwrap();
        assert_eq!(file.status_set_at(), Some(3.0));

        let file: SessionFile = serde_json::from_str(r#"{"startedAt":1}"#).unwrap();
        assert_eq!(file.status_set_at(), Some(1.0));
    }

    /// A file with no `procStart` cannot be checked, so it is not trusted.
    #[test]
    fn a_file_without_proc_start_is_not_live() {
        let file: SessionFile = serde_json::from_str(r#"{"pid":1}"#).unwrap();
        assert!(file.row(0.0).is_none());
    }

    #[test]
    fn sort_key_orders_by_session_then_pane_then_pid() {
        let mut a = row();
        a.session = "alpha".into();
        let b = row();
        assert!(a.sort_key() < b.sort_key());
    }
}
