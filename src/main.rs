mod agent;
mod human;
mod proc;

use clap::Parser;
use std::io::Write;

#[derive(clap::Parser)]
#[command(
    name = "claude-ps",
    version,
    about = "ps for Claude Code: all running agents, as JSON",
    long_about = "\
ps for Claude Code. Prints all running agents as a JSON array on stdout, one
object per agent:

  status              what Claude reports, verbatim (busy, idle, waiting, ...),
                      or null if Claude reports none
  status_age          whole seconds in that status, or 0 if no timestamp
                      dates it
  zellij              {session, pane}, or null if the agent is not in zellij
  name                Claude's own label for the session
  name_source         who chose the name (user, derived, ...), or null
  pid                 the process id
  session_id          Claude's session uuid, which is also the transcript name
  session_started_at  epoch seconds when the session started, or 0 if unknown
  cwd                 the working directory of the agent
  permission_mode     the mode the command line asks for, or null

The status vocabulary is open and changes with the version of Claude Code. Do
not compare the status against a fixed set of values. The same holds for
name_source and permission_mode.

name_source says whether the name carries information. A derived name is the
basename of the cwd and a suffix, so a consumer that shows the cwd shows it
twice. Only user and peer are a name that somebody chose.

permission_mode is the launch of the agent and not the mode it runs under now.
A command line does not change, and a person cycles the mode during a session.

status_age and session_started_at answer different questions. status_age is the
time in the current status. session_started_at is the time when the session
started. A new session and a session that completed a turn are both idle with a
small status_age. Only session_started_at makes them different.

Agents with a stale session file do not appear: the pid must be alive, and the
start time of the process must agree with the session file.

The order is by pid. This order is for stable diffs. Sort the agents again to
show them to a person.

--format text prints a table for a person instead: one line for each agent, the
timestamps as durations, the home directory as ~, and a ~ after a name that
Claude derived rather than a person chose. That table has no
stability rule, and its order is by name. Read the JSON from a program."
)]
struct Cli {
    #[arg(short, long, value_enum, default_value_t = Format::Json)]
    format: Format,
}

#[derive(clap::ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Json,
    Text,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    match run(cli.format) {
        Ok(output) => match write_stdout(&output) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("claude-ps: could not write to stdout: {error}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("claude-ps: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn write_stdout(output: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout();
    stdout.write_all(output.as_bytes())?;
    stdout.flush()
}

fn run(format: Format) -> Result<String, String> {
    let now_secs = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "the system clock is before the epoch".to_string())?
            .as_secs(),
    )
    .map_err(|_| "the system clock is too far in the future".to_string())?;

    let home = home()?;
    let mut agents = collect(&sessions_dir(&home), now_secs);

    agents.sort_by_key(|agent| agent.pid);

    match format {
        Format::Json => {
            let mut out = serde_json::to_string_pretty(&agents)
                .map_err(|error| format!("could not serialise the agent list: {error}"))?;
            out.push('\n');
            Ok(out)
        }
        Format::Text => Ok(human::table(&agents, now_secs, &home)),
    }
}

fn home() -> Result<String, String> {
    std::env::var("HOME").map_err(|_| "HOME is not set".to_string())
}

fn sessions_dir(home: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(home)
        .join(".claude")
        .join("sessions")
}

fn collect(dir: &std::path::PathBuf, now_secs: i64) -> Vec<agent::Agent> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| std::fs::read_to_string(&path).ok())
        .filter_map(|text| serde_json::from_str::<agent::SessionFile>(&text).ok())
        .filter_map(|file| file.agent(now_secs))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_missing_sessions_directory_yields_no_agents() {
        let agents = super::collect(&std::path::PathBuf::from("/nonexistent/claude/sessions"), 0);
        assert!(agents.is_empty());
    }

    #[test]
    fn no_agents_is_an_empty_array_not_empty_output() {
        let empty: Vec<crate::agent::Agent> = Vec::new();
        assert_eq!(serde_json::to_string(&empty).unwrap(), "[]");
    }

    #[test]
    fn the_cli_definition_is_valid() {
        use clap::CommandFactory;

        super::Cli::command().debug_assert();
    }
}
