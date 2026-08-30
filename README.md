# claude-ps

`claude-ps` is `ps` for Claude Code. It prints all running agents as JSON.

## Installation

### Cargo

```sh
cargo build --release
ln -sf "$PWD/target/release/claude-ps" ~/.local/bin/claude-ps
```

### Nix

```sh
nix profile install github:lorenzolfm/claude-ps
```

## Requirements

Linux only. `claude-ps` reads `/proc/<pid>/environ` and `/proc/<pid>/stat`.
There is no procfs on darwin, so the flake does not build for darwin.

## Output

`claude-ps` writes a JSON array to stdout. Each object is one agent.

| Key | Content |
|---|---|
| `status` | What Claude reports, verbatim |
| `status_age` | Whole seconds in that status |
| `context` | `{tokens, as_of}` at the last assistant turn, or `null` |
| `zellij` | `{session, pane}`, or `null` if the agent is not in zellij |
| `name` | Claude's own label for the session |
| `pid` | The process id |
| `session_id` | Claude's session uuid, which is also the transcript name |
| `session_started_at` | Epoch seconds when the session started, or `0` if unknown |
| `cwd` | The working directory of the agent |

```console
$ claude-ps
[
  {
    "status": "waiting",
    "status_age": 35,
    "context": { "tokens": 187953, "as_of": 1788052221 },
    "zellij": { "session": "work", "pane": "0" },
    "name": "work-f8",
    "pid": 3318865,
    "session_id": "b08aacbc-…",
    "session_started_at": 1755000000,
    "cwd": "/home/you/Projects/work"
  },
  {
    "status": "busy",
    "status_age": 13,
    "context": null,
    "zellij": null,
    "name": "scratch-2c",
    "pid": 3129839,
    "session_id": "52b7681e-…",
    "session_started_at": 1755004000,
    "cwd": "/home/you/scratch"
  }
]
```

## Rules for consumers

**The status vocabulary is open.** Claude Code adds new values in new releases.
`claude-ps` passes the status through without a change. Do not compare the
status against a fixed set of values. A consumer that shows only the values it
knows hides live agents.

**`context` is a token count, and not a percentage.** Claude Code does not write
the size of the context window to disk. `claude-ps` reports only the number of
tokens. The `as_of` stamp gives the time of the last completed assistant turn.
An agent that is `busy` has added tokens after that time.

**`status_age` and `session_started_at` answer different questions.**
`status_age` is the time in the current status. `session_started_at` is the time
when the session started. A new session and a session that completed a turn are
both `idle` with a small `status_age`. Only `session_started_at` makes them
different.

**No agents is `[]`.** The output is always a JSON document, also when nothing
runs.

**Stale sessions do not appear.** Claude Code keeps the session file after the
agent stops. `claude-ps` shows an agent only if the pid is alive and the start
time of the process agrees with the session file.

**The exit status is 0 after a successful write.** It is also 0 if the reader
closes the pipe, which is what `head` does. It is 1 if `claude-ps` cannot write
its output, for example when the disk is full, and the message goes to stderr.

**The order is stable, and not presentational.** `claude-ps` sorts the agents by
pid. Sort the agents again if you show them to a person.

## Zellij support

If an agent runs in zellij, `claude-ps` reports the zellij session and pane in
the `zellij` key. The value is `null` for an agent outside zellij.

[luneta](https://github.com/lorenzolfm/luneta) uses this key. Its agent screen
shows all running agents. `Enter` attaches to the session and focuses the pane.

If you use luneta, install `claude-ps` on the `PATH` of the zellij **server**,
which is not the `PATH` of your shell. A server that started before the
installation does not find the tool. Restart the server, or set the
`agents_command` configuration key of luneta to the path of the executable.

## Other consumers

- [claude-tray](https://github.com/lorenzolfm/claude-tray) — a system tray
  applet that shows which sessions wait for you.

## License

MIT
