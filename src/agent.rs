//! The session file Claude Code writes, and the agent this tool prints for it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::proc;

/// `~/.claude/sessions/<pid>.json`, as much of it as this tool reads.
///
/// Every field is optional and unknown fields are ignored, which is deliberate rather than
/// defensive: this schema belongs to Claude Code and moves with its releases. A field that
/// disappears must cost one key, not the whole agent.
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
    /// Epoch milliseconds the session began. Dates the status as a last resort, **and** is a
    /// key in its own right: without it a consumer cannot tell a session that just launched
    /// from one that just finished a turn, because both read as `idle` with a small `age`.
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

    /// The agent for this file, or `None` if the process behind it is gone.
    pub fn agent(&self, now_secs: f64) -> Option<Agent> {
        let pid = self.pid?;
        if !self.is_live(pid) {
            return None;
        }
        Some(Agent {
            status: self.status.clone(),
            age: age_secs(now_secs, self.status_set_at()),
            zellij: Zellij::of(pid),
            name: self.name.clone(),
            pid,
            session_id: self.session_id.clone(),
            started_at: epoch_secs(self.started_at),
            cwd: self.cwd.clone(),
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
/// `0` epoch-milliseconds is the obvious shortcut and it renders every agent as roughly
/// fifty-seven years old, which looks like data rather than like breakage — so if Claude ever
/// renames these fields the failure is a column of `0`s, which is visibly wrong.
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

/// Epoch **seconds** for a timestamp Claude Code writes in milliseconds.
///
/// ⚠️ An absent timestamp is `0`, on the same reasoning as [`age_secs`]: it renders as 1970,
/// which reads as breakage rather than as data. It also fails in the safe direction for the one
/// thing this key exists for — a consumer suppressing just-launched sessions computes an
/// enormous session age, so it suppresses nothing rather than hiding a live agent.
pub fn epoch_secs(ms: Option<f64>) -> u64 {
    let Some(ms) = ms else {
        return 0;
    };
    let secs = ms / 1000.0;
    if secs.is_nan() || secs <= 0.0 {
        0
    } else {
        secs as u64
    }
}

/// Where an agent is sitting, when it is sitting in zellij at all.
///
/// 🔴 The two halves are **one object or nothing**, never one of each. Attaching to a session
/// and focusing a pane is a single act for a consumer, and a session without a pane would be an
/// address it cannot use. Nesting them makes that unrepresentable rather than merely documented
/// — which is what the old pair of `-` placeholders left to prose.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Zellij {
    pub session: String,
    pub pane: String,
}

impl Zellij {
    /// Read from the agent's own environment. This is the half of the join Claude does not
    /// write down: the session file says what an agent is doing and nothing about where it is.
    fn of(pid: u32) -> Option<Self> {
        match proc::zellij_of(pid) {
            (Some(session), Some(pane)) => Some(Zellij { session, pane }),
            _ => None,
        }
    }
}

/// One agent, as one JSON object.
///
/// 🔴 Key order here is the order they serialise in, and it is the reading order: what the
/// agent is doing, then how long, then where it is. Consumers address these **by name**, so
/// adding a key is not a breaking change the way appending a column was.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// Verbatim from Claude Code. `null` only if the file had no status at all.
    pub status: Option<String>,
    /// Whole seconds in the current status.
    pub age: u64,
    /// `null` when the agent is not inside zellij, which is an ordinary state and not a fault.
    pub zellij: Option<Zellij>,
    /// Claude's own derived label, **not** the zellij session name.
    pub name: Option<String>,
    pub pid: u32,
    pub session_id: Option<String>,
    /// Epoch seconds, and deliberately absolute where `age` is a duration: it answers *when did
    /// this session begin*, which does not go stale between this process reading it and a
    /// consumer using it.
    pub started_at: u64,
    pub cwd: Option<String>,
}

impl Agent {
    /// Deterministic, **not** presentational. Two runs a second apart diff cleanly; deciding
    /// what order a human should see them in belongs to whatever is doing the showing.
    ///
    /// Agents outside zellij sort last as a group, because the empty key sorts before every
    /// real session name and putting them first would bury the addressable ones.
    pub fn sort_key(&self) -> (bool, &str, &str, u32) {
        match &self.zellij {
            Some(z) => (false, z.session.as_str(), z.pane.as_str(), self.pid),
            None => (true, "", "", self.pid),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> Agent {
        Agent {
            status: Some("waiting".into()),
            age: 35,
            zellij: Some(Zellij {
                session: "work".into(),
                pane: "1".into(),
            }),
            name: Some("work-f8".into()),
            pid: 4242,
            session_id: Some("abc-123".into()),
            started_at: 1_755_000_000,
            cwd: Some("/home/you/src".into()),
        }
    }

    #[test]
    fn serialises_every_key_in_the_published_order() {
        let json = serde_json::to_string(&agent()).unwrap();
        assert_eq!(
            json,
            r#"{"status":"waiting","age":35,"zellij":{"session":"work","pane":"1"},"name":"work-f8","pid":4242,"session_id":"abc-123","started_at":1755000000,"cwd":"/home/you/src"}"#
        );
    }

    /// 🔴 The reason the pair is nested. An agent outside zellij has no address at all, and one
    /// `null` says that where two placeholders left a consumer to check both and agree on what
    /// the pair meant.
    #[test]
    fn an_agent_outside_zellij_is_one_null() {
        let mut agent = agent();
        agent.zellij = None;
        let value: Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["zellij"], Value::Null);
    }

    /// A cwd with whitespace needed a rule about column position under TSV. Under JSON it needs
    /// nothing: it is a string, and the encoder owns the escaping.
    #[test]
    fn a_cwd_with_whitespace_needs_no_special_handling() {
        let mut agent = agent();
        agent.cwd = Some("/home/you/my projects/thing".into());
        let value: Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["cwd"], "/home/you/my projects/thing");
    }

    /// 🔴 The whole reason this key exists: a session that just launched and one that just
    /// finished a turn are both `idle` with a small `age`, and only `started_at` separates them.
    #[test]
    fn started_at_separates_a_newborn_from_a_finished_turn() {
        let newborn: SessionFile =
            serde_json::from_str(r#"{"startedAt":1755000000000,"statusUpdatedAt":1755000002000}"#)
                .unwrap();
        let finished: SessionFile =
            serde_json::from_str(r#"{"startedAt":1754000000000,"statusUpdatedAt":1755000002000}"#)
                .unwrap();

        // Identical from `age` alone -- the only thing a consumer had before this key.
        assert_eq!(
            age_secs(1_755_000_012.0, newborn.status_set_at()),
            age_secs(1_755_000_012.0, finished.status_set_at())
        );

        // Separable once the launch time is carried too.
        assert_eq!(epoch_secs(newborn.started_at), 1_755_000_000);
        assert_eq!(epoch_secs(finished.started_at), 1_754_000_000);
    }

    /// ⚠️ Same reasoning as an undated age: 1970 reads as breakage, and a consumer computing a
    /// session age from it decides "not a newborn", which is the direction that shows an agent
    /// rather than hides one.
    #[test]
    fn an_undated_start_is_zero_not_now() {
        assert_eq!(epoch_secs(None), 0);
        assert_eq!(epoch_secs(Some(-1.0)), 0);
        assert_eq!(epoch_secs(Some(1_755_000_000_999.0)), 1_755_000_000);
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

    /// Unknown fields are ignored and missing ones are `None`, so a schema change costs a key
    /// rather than the agent.
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
        assert!(file.agent(0.0).is_none());
    }

    #[test]
    fn sort_key_orders_by_session_then_pane_then_pid() {
        let mut a = agent();
        a.zellij = Some(Zellij {
            session: "alpha".into(),
            pane: "1".into(),
        });
        assert!(a.sort_key() < agent().sort_key());
    }

    /// Outside-zellij agents group at the end rather than at the front, where an empty session
    /// name would otherwise put them.
    #[test]
    fn agents_outside_zellij_sort_last() {
        let mut outside = agent();
        outside.zellij = None;
        assert!(agent().sort_key() < outside.sort_key());
    }
}
