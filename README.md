# Linstruct

**Your AI instruction files, as each agent will actually read them — and what it will
silently drop.**

Every AI coding host reads instruction files its own way: `CLAUDE.md`, `AGENTS.md`,
`.cursor/rules/*.mdc`, `.github/copilot-instructions.md`, `GEMINI.md`, `.goosehints`,
`.kiro/steering/`… not the same names, not the same places, not the same limits, not the same
order. And when a limit is crossed, the host **cuts or ignores without saying so**: Codex
truncates its `AGENTS.md` chain mid-file past 32 KiB and writes a line in a log nobody reads;
Claude Code reads `AGENTS.md` only when there is no `CLAUDE.md` above it; Cursor ignores a
`.md` in `.cursor/rules` and a rule whose front matter is broken; Warp wants `AGENTS.md` in
capitals.

Linstruct rebuilds, for each host present on your machine and each project you work in, the
**stack of files the host would load** — in its order, references expanded, budget simulated
— and says what it would cut, ignore or shadow. Read-only, no network, secrets masked, every
rule sourced from the host's documentation or source code. Part of the
[Blackberrylabs](https://github.com/blackberrylabs) family; the
[control tower](https://github.com/blackberrylabs/blackberrylabs) reads its report.

## What it finds

| Severity | Kinds |
|---|---|
| **1 · act now** | `cut` — a host cuts or skips content, and its docs or source prove it (Codex past 32 KiB, Claude Code past 4 MiB, Goose past its import budget) · `secret` — a credential in clear, sent with every request |
| **2 · review** | `over-limit` — past a documented limit with undocumented consequence (Devin 12,000 / 6,000 chars) · `ignored` — a file this host never reads · `dead-import` — an `@path` that brings nothing in · `frontmatter` — a rule that does not apply as written |
| **3 · good to know** | `shadowed` — another file takes its place · `over-length` — past the host's recommendation or near its budget · `duplicate` — the same text loaded twice · `comments` — HTML comments sent to the model · `legacy` — an older name still read |
| **4 · information** | `empty` · `unreadable` |

Nineteen hosts, each from a sourced sheet in [`hosts/`](hosts): Claude Code, Codex CLI,
Cursor, GitHub Copilot, Gemini CLI, Qwen Code, Cline, Roo Code, Kilo Code, OpenCode, Goose,
Zed, Kiro, Amp, Aider, Devin Desktop, JetBrains Junie, JetBrains AI Assistant, Warp, Trae.
Hosts that are not on your machine are still computed and shown in the app as "what if" —
useful for an `AGENTS.md` shared with people who use other tools — but never counted in the
summary nor sent to the tower.

## Use

```sh
linstruct                      # the findings, most serious first, for the projects your machine knows
linstruct -p ~/code/myrepo     # one project
linstruct stacks               # what each host loads, file by file, with its budget
linstruct stacks --host codex
linstruct hosts                # the hosts, their presence, where their rules come from
linstruct --all-hosts          # include the hosts not on this machine
linstruct --json               # everything, for scripts
linstruct report               # bbl-report/1 for the Blackberrylabs control tower
linstruct-app                  # the desktop app
```

Projects: the ones you name with `-p`, else the projects Claude Code knows on this machine
(`~/.claude.json`), else the current directory. The host's working directory is taken as the
project root; files in subdirectories are shown as "on demand" or "when started there".

## What Linstruct does not do

It does not judge the meaning of your instructions (no semantic contradictions, no style
review), does not change any file, does not fetch the remote instructions some hosts accept
(it lists them), and does not read the rules that are not files (Cursor User Rules, Warp
Global Rules). What the host does with a broken front matter or past an undocumented limit is
said to be undocumented, not guessed.

## Promises

Read-only (nothing written outside `~/.config/linstruct/`, the app's preferences); no network;
secrets masked the moment a line is read and never written; actions shown, never run; every
host rule cites its source, its date and the words it was read from (`docs/hosts.md`).
`#![forbid(unsafe_code)]`; `tests/promises.rs` checks the source for writes and network calls
and the report for conformance.

## Build

```sh
cargo test
cargo build --release          # target/release/linstruct, linstruct-app
./packaging/build.sh           # .deb, .rpm, AppImage in dist/
```

Depends on [`bbl-hostrules`](https://github.com/blackberrylabs/bbl-hostrules) (locations,
readers, secret masking), [`bbl-report`](https://github.com/blackberrylabs/bbl-report) (the
tower's contract) and [`bbl-ui`](https://github.com/blackberrylabs/bbl-ui) (the family's face).

## Status

0.1.0 in preparation. License MIT — Alexandre Do.
