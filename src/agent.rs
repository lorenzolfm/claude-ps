#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub pid: Option<u32>,
    pub proc_start: Option<serde_json::Value>,
    pub pid_domain: Option<String>,
    pub status: Option<String>,
    pub name: Option<String>,
    pub name_source: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub status_updated_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub started_at: Option<i64>,
}

impl SessionFile {
    fn live_pid(&self, pid: u32) -> Option<crate::proc::LivePid> {
        if !self.is_local() {
            return None;
        }
        let recorded = self.proc_start.as_ref().map(json_scalar)?;
        crate::proc::live_pid(pid, &recorded)
    }

    fn is_local(&self) -> bool {
        is_local_domain(self.pid_domain.as_deref(), crate::proc::local_pid_domain())
    }

    fn status_set_at(&self) -> Option<i64> {
        self.status_updated_at
            .or(self.updated_at)
            .or(self.started_at)
    }

    pub fn agent(&self, now_secs: i64) -> Option<Agent> {
        let pid = self.live_pid(self.pid?)?;
        Some(Agent {
            status: Text::word(self.status.as_deref()),
            status_age: status_age_secs(now_secs, self.status_set_at()),
            zellij: Zellij::of(pid),
            name: Name::of(self.name.as_deref(), self.name_source.as_deref()),
            pid,
            session_id: Text::verbatim(self.session_id.as_deref()),
            session_started_at: epoch_secs(self.started_at),
            cwd: Text::verbatim(self.cwd.as_deref()),
            permission_mode: Text::word(crate::proc::permission_mode(pid).as_deref()),
        })
    }
}

fn is_local_domain(recorded: Option<&str>, local: Option<&str>) -> bool {
    match (recorded, local) {
        (Some(recorded), Some(local)) => recorded == local,
        _ => true,
    }
}

fn json_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn status_age_secs(now_secs: i64, status_set_at_ms: Option<i64>) -> Option<u64> {
    let set_at_ms = status_set_at_ms?;
    let elapsed_ms = now_secs.saturating_mul(1000).saturating_sub(set_at_ms);
    Some(u64::try_from(elapsed_ms / 1000).unwrap_or(0))
}

pub fn epoch_secs(ms: Option<i64>) -> Option<u64> {
    Some(u64::try_from(ms?).ok()? / 1000)
}

fn zero_when_unknown<S: serde::Serializer>(
    secs: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(secs.unwrap_or(0))
}

#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Zellij {
    pub session: Text,
    pub pane: Text,
}

impl Zellij {
    fn of(pid: crate::proc::LivePid) -> Option<Self> {
        Self::address(crate::proc::zellij_of(pid))
    }

    pub(crate) fn address(vars: (Option<String>, Option<String>)) -> Option<Self> {
        let (session, pane) = vars;
        Some(Zellij {
            session: Text::verbatim(session.as_deref())?,
            pane: Text::verbatim(pane.as_deref())?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Name {
    pub text: Text,
    pub source: Option<Text>,
}

impl Name {
    pub fn of(name: Option<&str>, source: Option<&str>) -> Option<Self> {
        Some(Name {
            text: Text::verbatim(name)?,
            source: Text::word(source),
        })
    }
}

fn name_and_source<S: serde::Serializer>(
    name: &Option<Name>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;

    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("name", &name.as_ref().map(|name| &name.text))?;
    map.serialize_entry(
        "name_source",
        &name.as_ref().and_then(|name| name.source.as_ref()),
    )?;
    map.end()
}

#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct Text(String);

impl Text {
    pub fn word(raw: Option<&str>) -> Option<Self> {
        raw.map(str::trim)
            .filter(|word| !word.is_empty())
            .map(|word| Self(word.to_string()))
    }

    pub fn verbatim(raw: Option<&str>) -> Option<Self> {
        raw.filter(|value| !value.is_empty())
            .map(|value| Self(value.to_string()))
    }
}

impl std::ops::Deref for Text {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub status: Option<Text>,
    #[serde(serialize_with = "zero_when_unknown")]
    pub status_age: Option<u64>,
    pub zellij: Option<Zellij>,
    #[serde(flatten, serialize_with = "name_and_source")]
    pub name: Option<Name>,
    pub pid: crate::proc::LivePid,
    pub session_id: Option<Text>,
    #[serde(serialize_with = "zero_when_unknown")]
    pub session_started_at: Option<u64>,
    pub cwd: Option<Text>,
    pub permission_mode: Option<Text>,
}

#[cfg(test)]
mod tests {
    fn agent() -> super::Agent {
        super::Agent {
            status: super::Text::word(Some("waiting")),
            status_age: Some(35),
            zellij: address("work", "1"),
            name: super::Name::of(Some("work-f8"), Some("user")),
            pid: crate::proc::LivePid::unchecked(4242),
            session_id: super::Text::verbatim(Some("abc-123")),
            session_started_at: Some(1_755_000_000),
            cwd: super::Text::verbatim(Some("/home/you/src")),
            permission_mode: super::Text::word(Some("plan")),
        }
    }

    fn address(session: &str, pane: &str) -> Option<super::Zellij> {
        super::Zellij::address((Some(session.to_string()), Some(pane.to_string())))
    }

    #[test]
    fn serialises_every_key_in_the_published_order() {
        let json = serde_json::to_string(&agent()).unwrap();
        assert_eq!(
            json,
            r#"{"status":"waiting","status_age":35,"zellij":{"session":"work","pane":"1"},"name":"work-f8","name_source":"user","pid":4242,"session_id":"abc-123","session_started_at":1755000000,"cwd":"/home/you/src","permission_mode":"plan"}"#
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
    fn a_half_of_an_address_with_nothing_in_it_is_a_half_that_is_missing() {
        assert_eq!(address("", "1"), None);
        assert_eq!(address("work", ""), None);
        assert_eq!(address("", ""), None);
        assert_eq!(super::Zellij::address((None, None)), None);
    }

    #[test]
    fn a_whole_address_survives() {
        let zellij = address("work", "1").expect("an address");
        assert_eq!(&*zellij.session, "work");
        assert_eq!(&*zellij.pane, "1");
    }

    #[test]
    fn a_session_name_keeps_the_space_that_is_part_of_it() {
        let zellij = address(" my work ", " 1 ").expect("an address");
        assert_eq!(&*zellij.session, " my work ");
        assert_eq!(&*zellij.pane, " 1 ");
    }

    #[test]
    fn an_agent_whose_address_was_empty_is_one_null_too() {
        let mut agent = agent();
        agent.zellij = address("", "");
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["zellij"], serde_json::Value::Null);
    }

    #[test]
    fn a_cwd_with_whitespace_needs_no_special_handling() {
        let mut agent = agent();
        agent.cwd = super::Text::verbatim(Some("/home/you/my projects/thing"));
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["cwd"], "/home/you/my projects/thing");
    }

    #[test]
    fn a_status_with_nothing_in_it_is_absent_and_not_an_empty_word() {
        for raw in ["", " ", "\t", "\n  "] {
            assert_eq!(super::Text::word(Some(raw)), None);
        }
        assert_eq!(super::Text::word(None), None);

        let mut agent = agent();
        agent.status = super::Text::word(Some("  "));
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["status"], serde_json::Value::Null);
    }

    #[test]
    fn a_status_word_is_carried_through_verbatim() {
        for raw in ["waiting", "idle", "busy", "shell", "somethingNew"] {
            let word = super::Text::word(Some(raw)).expect("a word");
            assert_eq!(&*word, raw);
            assert_eq!(serde_json::to_string(&word).unwrap(), format!(r#""{raw}""#));
        }
        assert_eq!(&*super::Text::word(Some(" busy ")).unwrap(), "busy");
    }

    #[test]
    fn a_key_with_nothing_in_it_is_absent_for_all_of_them() {
        let mut agent = agent();
        agent.name = super::Name::of(Some(""), Some(""));
        agent.session_id = super::Text::verbatim(Some(""));
        agent.cwd = super::Text::verbatim(Some(""));
        agent.permission_mode = super::Text::word(Some(""));

        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        for key in [
            "name",
            "name_source",
            "session_id",
            "cwd",
            "permission_mode",
        ] {
            assert_eq!(value[key], serde_json::Value::Null, "{key}");
        }
    }

    #[test]
    fn a_word_is_trimmed_and_a_name_is_not() {
        assert_eq!(&*super::Text::word(Some("  plan  ")).unwrap(), "plan");
        assert_eq!(
            &*super::Text::verbatim(Some("/home/you/two spaces / ")).unwrap(),
            "/home/you/two spaces / "
        );
        assert_eq!(&*super::Text::verbatim(Some(" ")).unwrap(), " ");
    }

    #[test]
    fn a_narrowed_value_is_the_same_on_the_wire() {
        let json = serde_json::to_string(&super::Text::verbatim(Some("work-f8"))).unwrap();
        assert_eq!(json, r#""work-f8""#);
    }

    #[test]
    fn the_session_start_separates_a_newborn_from_a_finished_turn() {
        let newborn: super::SessionFile =
            serde_json::from_str(r#"{"startedAt":1755000000000,"statusUpdatedAt":1755000002000}"#)
                .unwrap();
        let finished: super::SessionFile =
            serde_json::from_str(r#"{"startedAt":1754000000000,"statusUpdatedAt":1755000002000}"#)
                .unwrap();

        assert_eq!(
            super::status_age_secs(1_755_000_012, newborn.status_set_at()),
            super::status_age_secs(1_755_000_012, finished.status_set_at())
        );

        assert_eq!(super::epoch_secs(newborn.started_at), Some(1_755_000_000));
        assert_eq!(super::epoch_secs(finished.started_at), Some(1_754_000_000));
    }

    #[test]
    fn an_undated_session_start_is_absent_and_not_now() {
        assert_eq!(super::epoch_secs(None), None);
        assert_eq!(super::epoch_secs(Some(-1)), None);
        assert_eq!(
            super::epoch_secs(Some(1_755_000_000_999)),
            Some(1_755_000_000)
        );
    }

    #[test]
    fn the_status_age_is_whole_seconds_since_the_status_was_set() {
        assert_eq!(super::status_age_secs(1_000, Some(940_500)), Some(59));
    }

    #[test]
    fn the_status_age_clamps_a_future_timestamp_to_zero() {
        assert_eq!(super::status_age_secs(1_000, Some(9_999_000)), Some(0));
    }

    #[test]
    fn an_undated_status_and_a_status_of_this_second_are_not_the_same_age() {
        assert_eq!(super::status_age_secs(1_755_000_000, None), None);
        assert_eq!(
            super::status_age_secs(1_755_000_000, Some(1_755_000_000_000)),
            Some(0)
        );
    }

    #[test]
    fn an_unknown_timestamp_is_zero_on_the_wire() {
        let mut agent = agent();
        agent.status_age = None;
        agent.session_started_at = None;
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["status_age"], 0);
        assert_eq!(value["session_started_at"], 0);
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
        assert!(file.agent(0).is_none());
    }

    #[test]
    fn a_pid_from_another_domain_is_not_local() {
        assert!(!super::is_local_domain(
            Some("linux:0123:pid:[1]"),
            Some("linux:4567:pid:[4026531836]")
        ));
    }

    #[test]
    fn a_pid_from_this_domain_is_local() {
        let domain = "linux:0123:pid:[4026531836]";
        assert!(super::is_local_domain(Some(domain), Some(domain)));
    }

    #[test]
    fn a_file_without_a_domain_is_accepted() {
        assert!(super::is_local_domain(None, Some("linux:0123:pid:[1]")));

        let file: super::SessionFile = serde_json::from_str(r#"{"pid":1}"#).unwrap();
        assert!(file.is_local());
    }

    #[test]
    fn a_machine_that_cannot_say_its_domain_accepts_every_file() {
        assert!(super::is_local_domain(Some("linux:0123:pid:[1]"), None));
        assert!(super::is_local_domain(None, None));
    }

    #[test]
    fn a_source_without_a_name_is_no_name_at_all() {
        assert_eq!(super::Name::of(None, Some("derived")), None);
        assert_eq!(super::Name::of(Some(""), Some("derived")), None);

        let mut agent = agent();
        agent.name = super::Name::of(None, Some("derived"));
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["name"], serde_json::Value::Null);
        assert_eq!(value["name_source"], serde_json::Value::Null);
    }

    #[test]
    fn a_name_without_a_source_is_still_a_name() {
        let name = super::Name::of(Some("work-f8"), None).expect("a name");
        assert_eq!(&*name.text, "work-f8");
        assert_eq!(name.source, None);
    }

    #[test]
    fn the_name_source_is_carried_through_verbatim() {
        for source in [
            "user",
            "peer",
            "derived",
            "collision",
            "auto",
            "hook",
            "somethingNew",
        ] {
            let file: super::SessionFile =
                serde_json::from_str(&format!(r#"{{"nameSource":"{source}"}}"#)).unwrap();
            assert_eq!(file.name_source.as_deref(), Some(source));
        }

        let file: super::SessionFile = serde_json::from_str(r#"{"name":"x"}"#).unwrap();
        assert_eq!(file.name_source, None);
    }
}
