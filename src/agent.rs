//! The session file Claude Code writes, and the agent this tool prints for it.

/// `~/.claude/sessions/<pid>.json`, as much of it as this tool reads.
///
/// All fields are optional, and unknown fields are ignored. This schema belongs to Claude Code
/// and changes with its releases. A field that goes away costs one key, and not the agent.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub pid: Option<u32>,
    /// Compared against `/proc/<pid>/stat` to find if the process is alive. Untyped, because
    /// Claude Code can write this value as a number or as a string.
    pub proc_start: Option<serde_json::Value>,
    /// Passed through verbatim. The vocabulary is open and changes with the version of Claude
    /// Code, so this tool does not compare the status against a known set of values.
    pub status: Option<String>,
    /// Claude's own label for the session, for example `zellij-f8`. This is the basename of the
    /// cwd and a suffix. It is not the zellij session name.
    pub name: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    /// Epoch milliseconds. The first of these three fields that is present dates the status.
    pub status_updated_at: Option<i64>,
    pub updated_at: Option<i64>,
    /// Epoch milliseconds when the session started. It dates the status as a last resort, and it
    /// is also an output key: a new session and a session that completed a turn are both `idle`
    /// with a small `status_age`, and only this field makes them different.
    pub started_at: Option<i64>,
}

impl SessionFile {
    /// Whether the pid in this file is still the process that wrote the file.
    ///
    /// Both conditions are necessary. Linux recycles pids, so a stale file can name a pid that
    /// now belongs to a different process. The start time makes the check exact.
    fn is_live(&self, pid: u32) -> bool {
        let Some(recorded) = self.proc_start.as_ref().map(json_scalar) else {
            return false;
        };
        crate::proc::start_time(pid).is_some_and(|actual| actual == recorded)
    }

    /// Milliseconds since the epoch that the current status was set, if it is known at all.
    fn status_set_at(&self) -> Option<i64> {
        self.status_updated_at
            .or(self.updated_at)
            .or(self.started_at)
    }

    /// The agent for this file, or `None` if the process behind it is gone.
    pub fn agent(&self, now_secs: i64, home: &str) -> Option<Agent> {
        let pid = self.pid?;
        if !self.is_live(pid) {
            return None;
        }
        Some(Agent {
            status: self.status.clone(),
            status_age: status_age_secs(now_secs, self.status_set_at()),
            // Needs the cwd and the session id. This join is a guess and not a proof.
            // See `transcript`.
            context: match (self.cwd.as_deref(), self.session_id.as_deref()) {
                (Some(cwd), Some(session_id)) => {
                    crate::transcript::context_of(home, cwd, session_id)
                }
                _ => None,
            },
            zellij: Zellij::of(pid),
            name: self.name.clone(),
            pid,
            session_id: self.session_id.clone(),
            session_started_at: epoch_secs(self.started_at),
            cwd: self.cwd.clone(),
        })
    }
}

/// A JSON scalar as the shortest string that round-trips it, so a `procStart` of `987654` and a
/// `procStart` of `"987654"` compare equal.
fn json_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Whole seconds in the current status.
///
/// An unknown timestamp gives a status age of `0`. A missing field read as epoch millisecond `0`
/// would show every agent in its status for approximately 57 years, which looks like data and not
/// like a fault.
pub fn status_age_secs(now_secs: i64, status_set_at_ms: Option<i64>) -> u64 {
    let Some(set_at_ms) = status_set_at_ms else {
        return 0;
    };
    let elapsed_ms = now_secs.saturating_mul(1000).saturating_sub(set_at_ms);
    u64::try_from(elapsed_ms / 1000).unwrap_or(0)
}

/// Epoch seconds for a timestamp that Claude Code writes in milliseconds.
///
/// An absent timestamp gives `0`, for the same reason as [`status_age_secs`]. A consumer that
/// hides new sessions then computes a very large session age, so it hides nothing.
pub fn epoch_secs(ms: Option<i64>) -> u64 {
    let Some(ms) = ms else {
        return 0;
    };
    u64::try_from(ms / 1000).unwrap_or(0)
}

/// Where an agent runs in zellij.
///
/// The two fields are one object or nothing, and never one of the two. A session without a pane
/// is not an address that a consumer can use.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Zellij {
    pub session: String,
    pub pane: String,
}

impl Zellij {
    /// Read from the environment of the agent. The session file gives the status of an agent
    /// and says nothing about zellij.
    fn of(pid: u32) -> Option<Self> {
        match crate::proc::zellij_of(pid) {
            (Some(session), Some(pane)) => Some(Zellij { session, pane }),
            _ => None,
        }
    }
}

/// Agent information that is printed to stdout.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub status: Option<String>,
    pub status_age: u64,
    pub context: Option<crate::transcript::Context>,
    pub zellij: Option<Zellij>,
    pub name: Option<String>,
    pub pid: u32,
    pub session_id: Option<String>,
    pub session_started_at: u64,
    pub cwd: Option<String>,
}

impl Agent {
    /// A stable order, and not an order for a person: two runs one second apart give a small
    /// diff. Agents outside zellij sort last as a group.
    pub fn sort_key(&self) -> (bool, &str, &str, u32) {
        match &self.zellij {
            Some(z) => (false, z.session.as_str(), z.pane.as_str(), self.pid),
            None => (true, "", "", self.pid),
        }
    }
}

#[cfg(test)]
mod tests {
    fn agent() -> super::Agent {
        super::Agent {
            status: Some("waiting".into()),
            status_age: 35,
            context: Some(crate::transcript::Context {
                tokens: 187_953,
                as_of: 1_788_052_221,
            }),
            zellij: Some(super::Zellij {
                session: "work".into(),
                pane: "1".into(),
            }),
            name: Some("work-f8".into()),
            pid: 4242,
            session_id: Some("abc-123".into()),
            session_started_at: 1_755_000_000,
            cwd: Some("/home/you/src".into()),
        }
    }

    #[test]
    fn serialises_every_key_in_the_published_order() {
        let json = serde_json::to_string(&agent()).unwrap();
        assert_eq!(
            json,
            r#"{"status":"waiting","status_age":35,"context":{"tokens":187953,"as_of":1788052221},"zellij":{"session":"work","pane":"1"},"name":"work-f8","pid":4242,"session_id":"abc-123","session_started_at":1755000000,"cwd":"/home/you/src"}"#
        );
    }

    #[test]
    fn an_agent_outside_zellij_is_one_null() {
        let mut agent = agent();
        agent.zellij = None;
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["zellij"], serde_json::Value::Null);
    }

    #[test]
    fn a_cwd_with_whitespace_needs_no_special_handling() {
        let mut agent = agent();
        agent.cwd = Some("/home/you/my projects/thing".into());
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["cwd"], "/home/you/my projects/thing");
    }

    #[test]
    fn the_session_start_separates_a_newborn_from_a_finished_turn() {
        let newborn: super::SessionFile =
            serde_json::from_str(r#"{"startedAt":1755000000000,"statusUpdatedAt":1755000002000}"#)
                .unwrap();
        let finished: super::SessionFile =
            serde_json::from_str(r#"{"startedAt":1754000000000,"statusUpdatedAt":1755000002000}"#)
                .unwrap();

        // Identical from `status_age` alone -- the only thing a consumer had before this key.
        assert_eq!(
            super::status_age_secs(1_755_000_012, newborn.status_set_at()),
            super::status_age_secs(1_755_000_012, finished.status_set_at())
        );

        // Separable once the launch time is carried too.
        assert_eq!(super::epoch_secs(newborn.started_at), 1_755_000_000);
        assert_eq!(super::epoch_secs(finished.started_at), 1_754_000_000);
    }

    #[test]
    fn an_undated_session_start_is_zero_not_now() {
        assert_eq!(super::epoch_secs(None), 0);
        assert_eq!(super::epoch_secs(Some(-1)), 0);
        assert_eq!(super::epoch_secs(Some(1_755_000_000_999)), 1_755_000_000);
    }

    #[test]
    fn the_status_age_is_whole_seconds_since_the_status_was_set() {
        assert_eq!(super::status_age_secs(1_000, Some(940_500)), 59);
    }

    #[test]
    fn the_status_age_clamps_a_future_timestamp_to_zero() {
        assert_eq!(super::status_age_secs(1_000, Some(9_999_000)), 0);
    }

    #[test]
    fn the_status_age_of_an_undated_status_is_zero_not_the_epoch() {
        assert_eq!(super::status_age_secs(1_755_000_000, None), 0);
    }

    #[test]
    fn proc_start_compares_equal_as_number_or_string() {
        assert_eq!(super::json_scalar(&serde_json::json!(987654)), "987654");
        assert_eq!(super::json_scalar(&serde_json::json!("987654")), "987654");
    }

    #[test]
    fn session_file_tolerates_a_moving_schema() {
        let file: super::SessionFile =
            serde_json::from_str(r#"{"pid":7,"status":"shell","somethingNew":true,"cwd":"/tmp"}"#)
                .expect("unknown fields must not be an error");
        assert_eq!(file.pid, Some(7));
        assert_eq!(file.status.as_deref(), Some("shell"));
        assert_eq!(file.name, None);
        assert_eq!(file.status_set_at(), None);
    }

    #[test]
    fn status_set_at_prefers_the_most_specific_stamp() {
        let file: super::SessionFile =
            serde_json::from_str(r#"{"statusUpdatedAt":3,"updatedAt":2,"startedAt":1}"#).unwrap();
        assert_eq!(file.status_set_at(), Some(3));

        let file: super::SessionFile = serde_json::from_str(r#"{"startedAt":1}"#).unwrap();
        assert_eq!(file.status_set_at(), Some(1));
    }

    #[test]
    fn a_file_without_proc_start_is_not_live() {
        let file: super::SessionFile = serde_json::from_str(r#"{"pid":1}"#).unwrap();
        assert!(file.agent(0, "/home/you").is_none());
    }

    #[test]
    fn sort_key_orders_by_session_then_pane_then_pid() {
        let mut a = agent();
        a.zellij = Some(super::Zellij {
            session: "alpha".into(),
            pane: "1".into(),
        });
        assert!(a.sort_key() < agent().sort_key());
    }

    #[test]
    fn agents_outside_zellij_sort_last() {
        let mut outside = agent();
        outside.zellij = None;
        assert!(agent().sort_key() < outside.sort_key());
    }
}
