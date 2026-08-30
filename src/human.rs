//! The same agents, as a table for a person.
//!
//! This format is for eyes only. It has no stability rule: a column can move, and a value can
//! become shorter. A consumer reads the JSON.

/// The agents as a table between rules, or `no agents` if the list is empty.
///
/// The order is by name and then by pid, and not the pid order of the JSON. The JSON order is
/// for a small diff. A person looks for a name.
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

/// One cell for each column, in the order of [`HEADER`].
type Row = [String; COLUMNS];

const COLUMNS: usize = 7;

const HEADER: [&str; COLUMNS] = ["NAME", "STATUS", "AGE", "ELAPSED", "CWD", "PID", "ZELLIJ"];

/// The columns that hold a number or a duration. They are aligned to the right, so that the
/// digits of one column are above each other and two agents compare by eye.
const RIGHT_ALIGNED: [bool; COLUMNS] = [false, false, true, true, false, true, false];

fn row(agent: &crate::agent::Agent, now_secs: i64, home: &str) -> Row {
    [
        text(agent.name.as_deref()),
        text(agent.status.as_deref()),
        duration(agent.status_age),
        elapsed(agent.session_started_at, now_secs),
        agent
            .cwd
            .as_deref()
            .map_or_else(missing, |cwd| tilde(cwd, home)),
        agent.pid.to_string(),
        agent.zellij.as_ref().map_or_else(missing, |zellij| {
            format!("{}:{}", zellij.session, zellij.pane)
        }),
    ]
}

/// The mark for a value that the session file does not have. An empty cell reads as a column
/// that ended, and this tool prints the missing value of every key.
fn missing() -> String {
    "-".to_string()
}

fn text(value: Option<&str>) -> String {
    value.map_or_else(missing, str::to_string)
}

/// The header between two rules, the rows, and a rule below, in the style of `tokei`.
///
/// Each column is as wide as its widest cell, the header included. The rules are as wide as the
/// full table, so the eye finds the right edge of the last column.
fn render(header: &Row, rows: &[Row]) -> String {
    let mut widths = [0usize; COLUMNS];
    for row in std::iter::once(header).chain(rows) {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }

    // One space before the first column, two between the columns.
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

/// One line of cells. The last column carries no padding to its right, so a long `zellij` name
/// adds no trailing spaces.
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

/// The home directory as `~`, because the same prefix on every line carries no information.
///
/// Only an exact match of a full path component. A home of `/home/you` does not shorten
/// `/home/younger`.
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

/// How long the session runs, from a start in epoch seconds.
///
/// A start of `0` is the unknown start of the JSON, and it prints as the missing value. A clock
/// that moved back gives a start in the future, which prints as `0s` and not as a large number.
fn elapsed(started_at: u64, now_secs: i64) -> String {
    if started_at == 0 {
        return missing();
    }
    let now = u64::try_from(now_secs).unwrap_or(0);
    duration(now.saturating_sub(started_at))
}

/// Whole seconds as the two largest units that are not zero, for example `2h 5m`.
///
/// Two units, because the second unit says if the first one is about to change, and a third unit
/// is noise at that scale.
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
            status: Some("waiting".into()),
            status_age: 35,
            zellij: Some(crate::agent::Zellij {
                session: "work".into(),
                pane: "1".into(),
            }),
            name: Some(name.into()),
            pid,
            session_id: Some("abc-123".into()),
            session_started_at: 1_755_000_000,
            cwd: Some("/home/you/src".into()),
        }
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
                "==========================================================",
                " NAME           STATUS   AGE  ELAPSED  CWD     PID  ZELLIJ",
                "==========================================================",
                " a-longer-name  waiting  35s   1m 40s  ~/src     7  work:1",
                " b              waiting  35s   1m 40s  ~/src  4242  work:1",
                "==========================================================",
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
        one.session_started_at = 0;
        let table = super::table(&[one], 1_755_000_100, "/home/you");
        assert_eq!(
            table.lines().nth(3).unwrap(),
            " -     -       35s        -  -      1  -"
        );
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
        assert_eq!(super::elapsed(0, 1_755_000_000), "-");
    }

    #[test]
    fn a_session_start_in_the_future_is_zero_and_not_a_large_number() {
        assert_eq!(super::elapsed(1_755_000_100, 1_755_000_000), "0s");
    }
}
