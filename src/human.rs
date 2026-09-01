pub fn table(agents: &[crate::agent::Agent], now_secs: i64, home: &str) -> String {
    if agents.is_empty() {
        return "no agents\n".to_string();
    }

    let mut agents: Vec<&crate::agent::Agent> = agents.iter().collect();
    agents.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.pid.cmp(&b.pid)));

    let rows: Vec<Row> = agents
        .into_iter()
        .map(|agent| row(agent, now_secs, home))
        .collect();
    render(&HEADER.map(str::to_string), &rows)
}

type Row = [String; COLUMNS];

const COLUMNS: usize = 8;

const HEADER: [&str; COLUMNS] = [
    "NAME", "STATUS", "AGE", "ELAPSED", "MODE", "CWD", "PID", "ZELLIJ",
];

const RIGHT_ALIGNED: [bool; COLUMNS] = [false, false, true, true, false, false, true, false];

fn row(agent: &crate::agent::Agent, now_secs: i64, home: &str) -> Row {
    [
        name(agent),
        text(agent.status.as_deref()),
        agent.status_age.map_or_else(missing, duration),
        elapsed(agent.session_started_at, now_secs),
        text(agent.permission_mode.as_deref()),
        agent
            .cwd
            .as_deref()
            .map_or_else(missing, |cwd| tilde(cwd, home)),
        agent.pid.to_string(),
        agent.zellij.as_ref().map_or_else(missing, |zellij| {
            format!("{}:{}", &*zellij.session, &*zellij.pane)
        }),
    ]
}

fn missing() -> String {
    "-".to_string()
}

fn name(agent: &crate::agent::Agent) -> String {
    let Some(name) = agent.name.as_ref() else {
        return missing();
    };
    let text = &*name.text;
    match name.source.as_deref() {
        None | Some("user" | "peer") => text.to_string(),
        Some(_) => format!("{text}~"),
    }
}

fn text(value: Option<&str>) -> String {
    value.map_or_else(missing, str::to_string)
}

fn render(header: &Row, rows: &[Row]) -> String {
    let mut widths = [0usize; COLUMNS];
    for row in std::iter::once(header).chain(rows) {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }

    let rule = "=".repeat(1 + widths.iter().sum::<usize>() + 2 * (COLUMNS - 1));

    let mut out = String::new();
    for line in [rule.clone(), line(header, &widths), rule.clone()] {
        out.push_str(&line);
        out.push('\n');
    }
    for row in rows {
        out.push_str(&line(row, &widths));
        out.push('\n');
    }
    out.push_str(&rule);
    out.push('\n');
    out
}

fn line(row: &Row, widths: &[usize; COLUMNS]) -> String {
    let mut out = String::from(" ");
    for (column, cell) in row.iter().enumerate() {
        let pad = widths[column] - cell.chars().count();
        let last = column == COLUMNS - 1;
        if RIGHT_ALIGNED[column] {
            out.extend(std::iter::repeat_n(' ', pad));
            out.push_str(cell);
        } else {
            out.push_str(cell);
            if !last {
                out.extend(std::iter::repeat_n(' ', pad));
            }
        }
        if !last {
            out.push_str("  ");
        }
    }
    out
}

fn tilde(path: &str, home: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    let Some(rest) = path.strip_prefix(home) else {
        return path.to_string();
    };
    if rest.is_empty() {
        return "~".to_string();
    }
    match rest.strip_prefix('/') {
        Some(rest) => format!("~/{rest}"),
        None => path.to_string(),
    }
}

fn elapsed(started_at: Option<u64>, now_secs: i64) -> String {
    let Some(started_at) = started_at else {
        return missing();
    };
    let now = u64::try_from(now_secs).unwrap_or(0);
    duration(now.saturating_sub(started_at))
}

fn duration(secs: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    let (major_size, major_unit, minor_size, minor_unit) = match secs {
        s if s < MINUTE => return format!("{s}s"),
        s if s < HOUR => (MINUTE, 'm', 1, 's'),
        s if s < DAY => (HOUR, 'h', MINUTE, 'm'),
        _ => (DAY, 'd', HOUR, 'h'),
    };
    let major = secs / major_size;
    let minor = secs % major_size / minor_size;
    if minor == 0 {
        return format!("{major}{major_unit}");
    }
    format!("{major}{major_unit} {minor}{minor_unit}")
}

#[cfg(test)]
mod tests {
    fn agent(name: &str, pid: u32) -> crate::agent::Agent {
        crate::agent::Agent {
            status: crate::agent::Text::word(Some("waiting")),
            status_age: Some(35),
            zellij: address("work", "1"),
            name: crate::agent::Name::of(Some(name), Some("user")),
            pid: crate::proc::LivePid::unchecked(pid),
            session_id: crate::agent::Text::verbatim(Some("abc-123")),
            session_started_at: Some(1_755_000_000),
            cwd: crate::agent::Text::verbatim(Some("/home/you/src")),
            permission_mode: None,
        }
    }

    fn address(session: &str, pane: &str) -> Option<crate::agent::Zellij> {
        crate::agent::Zellij::address((Some(session.to_string()), Some(pane.to_string())))
    }

    #[test]
    fn no_agents_says_so_and_prints_no_header() {
        assert_eq!(super::table(&[], 1_755_000_100, "/home/you"), "no agents\n");
    }

    #[test]
    fn the_table_sits_between_rules_that_span_it() {
        let agents = vec![agent("a-longer-name", 7), agent("b", 4_242)];
        let table = super::table(&agents, 1_755_000_100, "/home/you");
        let lines: Vec<&str> = table.lines().collect();

        assert_eq!(
            lines,
            [
                "================================================================",
                " NAME           STATUS   AGE  ELAPSED  MODE  CWD     PID  ZELLIJ",
                "================================================================",
                " a-longer-name  waiting  35s   1m 40s  -     ~/src     7  work:1",
                " b              waiting  35s   1m 40s  -     ~/src  4242  work:1",
                "================================================================",
            ]
        );
        assert!(lines.iter().all(|line| !line.ends_with(' ')));
    }

    #[test]
    fn a_rule_is_as_wide_as_the_widest_line() {
        let table = super::table(&[agent("x", 1)], 1_755_000_100, "/home/you");
        let lines: Vec<&str> = table.lines().collect();
        let widest = lines.iter().map(|line| line.len()).max().unwrap();
        assert_eq!(lines[0].len(), widest);
        assert_eq!(lines[0], lines[2]);
        assert_eq!(lines[0], *lines.last().unwrap());
    }

    #[test]
    fn the_order_is_by_name_and_not_by_pid() {
        let agents = vec![agent("zeta", 1), agent("alpha", 2)];
        let table = super::table(&agents, 1_755_000_100, "/home/you");
        let names: Vec<&str> = table
            .lines()
            .skip(3)
            .take(2)
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        assert_eq!(names, ["alpha", "zeta"]);
    }

    #[test]
    fn a_missing_value_is_a_dash_and_not_an_empty_cell() {
        let mut one = agent("x", 1);
        one.status = None;
        one.name = None;
        one.zellij = None;
        one.cwd = None;
        one.status_age = None;
        one.session_started_at = None;
        one.permission_mode = None;
        let table = super::table(&[one], 1_755_000_100, "/home/you");
        assert_eq!(
            table.lines().nth(3).unwrap(),
            " -     -         -        -  -     -      1  -"
        );
    }

    #[test]
    fn an_address_with_nothing_in_it_is_a_dash_and_not_a_bare_colon() {
        let mut one = agent("x", 1);
        one.zellij = address("", "");
        let table = super::table(&[one], 1_755_000_100, "/home/you");
        let row = table.lines().nth(3).unwrap();
        assert!(row.ends_with(" -"), "{row}");
        assert!(!row.contains(':'), "{row}");
    }

    #[test]
    fn a_derived_name_carries_a_mark_and_a_chosen_name_does_not() {
        let marked = |source: Option<&str>| {
            let mut one = agent("work-f8", 1);
            one.name = crate::agent::Name::of(Some("work-f8"), source);
            super::name(&one)
        };

        assert_eq!(marked(Some("user")), "work-f8");
        assert_eq!(marked(Some("peer")), "work-f8");
        assert_eq!(marked(None), "work-f8");

        assert_eq!(marked(Some("derived")), "work-f8~");
        assert_eq!(marked(Some("auto")), "work-f8~");
        assert_eq!(marked(Some("collision")), "work-f8~");
        assert_eq!(marked(Some("hook")), "work-f8~");
        assert_eq!(marked(Some("somethingNew")), "work-f8~");
    }

    #[test]
    fn an_agent_without_a_name_is_a_dash_and_never_a_lone_mark() {
        let mut one = agent("x", 1);
        one.name = crate::agent::Name::of(None, Some("derived"));
        assert_eq!(super::name(&one), "-");
    }

    #[test]
    fn a_name_with_nothing_in_it_is_the_same_dash() {
        let mut one = agent("x", 1);
        one.name = crate::agent::Name::of(Some(""), Some("derived"));
        assert_eq!(super::name(&one), "-");
    }

    #[test]
    fn a_cwd_with_nothing_in_it_is_a_dash_and_not_a_blank_cell() {
        let mut one = agent("x", 1);
        one.cwd = crate::agent::Text::verbatim(Some(""));
        let table = super::table(&[one], 1_755_000_100, "/home/you");
        assert_eq!(
            table.lines().nth(3).unwrap(),
            " x     waiting  35s   1m 40s  -     -      1  work:1"
        );
    }

    #[test]
    fn the_permission_mode_has_a_column() {
        let mut one = agent("x", 1);
        one.permission_mode = crate::agent::Text::word(Some("bypassPermissions"));
        let table = super::table(&[one], 1_755_000_100, "/home/you");
        assert!(table.contains("MODE"), "{table}");
        assert!(table.contains("bypassPermissions"), "{table}");
    }

    #[test]
    fn the_home_directory_becomes_a_tilde() {
        assert_eq!(super::tilde("/home/you/src", "/home/you"), "~/src");
        assert_eq!(super::tilde("/home/you", "/home/you"), "~");
    }

    #[test]
    fn tilde_needs_a_full_component_and_a_home() {
        assert_eq!(
            super::tilde("/home/younger/src", "/home/you"),
            "/home/younger/src"
        );
        assert_eq!(super::tilde("/srv/build", "/home/you"), "/srv/build");
        assert_eq!(super::tilde("/home/you/src", ""), "/home/you/src");
    }

    #[test]
    fn a_duration_carries_the_two_largest_units() {
        assert_eq!(super::duration(0), "0s");
        assert_eq!(super::duration(59), "59s");
        assert_eq!(super::duration(60), "1m");
        assert_eq!(super::duration(3_599), "59m 59s");
        assert_eq!(super::duration(3_600), "1h");
        assert_eq!(super::duration(7_530), "2h 5m");
        assert_eq!(super::duration(86_400), "1d");
        assert_eq!(super::duration(180_000), "2d 2h");
    }

    #[test]
    fn an_unknown_session_start_is_a_dash_and_not_57_years() {
        assert_eq!(super::elapsed(None, 1_755_000_000), "-");
    }

    #[test]
    fn a_session_start_in_the_future_is_zero_and_not_a_large_number() {
        assert_eq!(super::elapsed(Some(1_755_000_100), 1_755_000_000), "0s");
    }

    #[test]
    fn an_undated_status_is_a_dash_and_not_a_fresh_zero() {
        let mut undated = agent("x", 1);
        undated.status_age = None;
        let table = super::table(&[undated], 1_755_000_100, "/home/you");
        assert_eq!(
            table.lines().nth(3).unwrap(),
            " x     waiting    -   1m 40s  -     ~/src    1  work:1"
        );
    }
}
