# claude-ps

`claude-ps` is `ps` for Claude Code. It prints every running agent as JSON, and
joins each one to the zellij pane it is running in.

## The problem

Claude Code writes `~/.claude/sessions/<pid>.json` for each running agent. The
file carries the agent's status, its working directory, and the time the status
was set. It says nothing about zellij.

The agent's own environment carries `ZELLIJ_SESSION_NAME` and
`ZELLIJ_PANE_ID`. It says nothing about what the agent is doing.

The two halves share one thing: the process id. `claude-ps` joins them.

## Output

A JSON array on stdout, one object per agent.

| Key | Content |
|---|---|
| `status` | whatever Claude reports, verbatim |
| `age` | whole seconds spent in that status |
| `zellij` | `{session, pane}`, or `null` if the agent is not in zellij |
| `name` | Claude's own derived label, **not** the zellij session name |
| `pid` | the process id, and the key the two halves are joined on |
| `session_id` | Claude's session uuid, matching its transcript |
| `started_at` | epoch **seconds** the session began, or `0` if unknown |
| `cwd` | the agent's working directory |

```console
$ claude-ps
[
  {
    "status": "waiting",
    "age": 35,
    "zellij": { "session": "work", "pane": "0" },
    "name": "work-f8",
    "pid": 3318865,
    "session_id": "b08aacbc-…",
    "started_at": 1755000000,
    "cwd": "/home/you/Projects/work"
  },
  {
    "status": "busy",
    "age": 13,
    "zellij": null,
    "name": "scratch-2c",
    "pid": 3129839,
    "session_id": "52b7681e-…",
    "started_at": 1755004000,
    "cwd": "/home/you/scratch"
  }
]
```

The second agent is not in zellij, so its join is `null`.

**No agents is `[]`, never empty output.** A consumer deserialising this gets a
document in both cases, so "nothing is running" is never a parse error.

### `zellij` is one object, not two fields

Attaching to a session and focusing a pane is a **single act** for a consumer,
and a session without a pane is an address it cannot use. Nesting the pair makes
the half-answer unrepresentable rather than merely discouraged — where two
separate nullable fields would leave every consumer to check both and agree on
what a mismatch meant.

### `age` and `started_at` are not the same question

`age` is a duration in the current status; `started_at` is an absolute time the session began.
A consumer needs both, because **a session that has just launched and one that has just
finished a turn are both `idle` with a small `age`** — and a status bar that cannot tell them
apart nags you about every tab you open. Only the launch time separates them.

## Four things it does deliberately

### The status is passed through untouched

The status vocabulary is **open** and moves with Claude Code's version. A
release that emitted `busy`, `idle` and `waiting` was followed by one that also
emits `shell`, and there is no reason that is the last word.

So `claude-ps` never compares the status against a set it knows. Whatever
Claude wrote is what you get. **A consumer should not match on it either** — a
lookup table that renders only the statuses it recognises silently drops live
agents the day Claude invents one.

### Liveness is exact, not a guess

A session file outlives the agent that wrote it. Checking "is this pid alive"
is not enough, because pids are recycled: a stale file whose pid now belongs to
an unrelated process would hand you that process's zellij pane, and a consumer
would send you to a pane that has nothing to do with Claude.

So the check is both halves. The pid must be alive **and** have started when
the file says it did — field 22 of `/proc/<pid>/stat` against the file's
`procStart`. A recycled pid cannot pass.

### The order is for diffing, not for reading

Agents are sorted by session, then pane, then pid, with those outside zellij
last as a group. That is the only reason the order exists — and it is why the
output is pretty-printed rather than compact: one key per line means two runs a
second apart differ in the lines that actually changed.

Deciding what a human should see first is the consumer's job, and consumers
disagree — a picker wants the agent that is waiting on you at the top, a status
bar may want a fixed position per session so it does not jump.

### A missing timestamp is age zero

If none of `statusUpdatedAt`, `updatedAt` or `startedAt` is present, the age is
`0`.

Reading a missing field as epoch-millisecond `0` is the obvious shortcut and it
renders every agent as roughly fifty-seven years old, which reads as data rather
than as breakage. If Claude renames those fields, a column of `0`s is visibly
wrong.

## Compatibility

⚠️ **The output was TAB-separated columns through `0.1.0`, and is JSON from here on.**
There is no flag to get the old format back.

Positional columns made every *additive* change breaking: consumers checked the field
count exactly, so gaining `started_at` meant a picker that could not read a schema it
otherwise understood perfectly. Named keys invert that. A consumer ignores keys it does
not know, so a new one costs nothing and only a **removed or renamed** key is a hard
failure — which is the polarity you want, because adding is the common case.

Two other changes came with it: the `-` placeholder is gone in favour of `null`,
and the two zellij fields are one nested object.

## Consumers

- [zj-picker](https://github.com/lorenzolfm/zj-picker) — its agent screen lists
  every running agent and `Enter` attaches to the session **and** focuses the
  pane. It cannot do this join itself: a zellij plugin's wasi sandbox preopens
  only `/host`, `/data`, `/cache` and `/tmp`, so neither `~/.claude/sessions`
  nor `/proc` is readable from inside it.
- [claude-tray](https://github.com/lorenzolfm/claude-tray) — a system tray
  applet showing which sessions are waiting on you. It could read the registry
  itself and deliberately does not: one joiner, many consumers.

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

⚠️ If you use `zj-picker`, the zellij **server's** `PATH` is not your shell's.
The plugin looks this tool up by name there, exactly as it looks up `zoxide`, so
wherever you install it has to be on the `PATH` the server was started with — a
server that predates the install will not see it.

Where the server's `PATH` genuinely lacks it, the plugin's `agents_command`
configuration key names the executable to run instead. That is the supported
escape hatch: an install path compiled into the plugin is what once made it
unusable by anyone but its author.

## Linux only

The join reads `/proc/<pid>/environ` and `/proc/<pid>/stat`. There is no procfs
on darwin, where this would compile and then report no agents at all — so the
flake does not offer a darwin build rather than offering one that is quietly
useless.

## License

MIT
