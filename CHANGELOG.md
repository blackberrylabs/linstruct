# Changelog

## [Unreleased]

### Added

- First version of Linstruct: the instruction files of a machine as each AI host reads them.
  Nineteen hosts, each from a sourced sheet (`hosts/*.toml`): Claude Code, Codex CLI, Cursor,
  GitHub Copilot, Gemini CLI, Qwen Code, Cline, Roo Code, Kilo Code, OpenCode, Goose, Zed,
  Kiro, Amp, Aider, Devin Desktop, JetBrains Junie, JetBrains AI Assistant, Warp, Trae.
- Thirteen kinds of finding, on the four severities of the Blackberrylabs family:
  - **1 act now**: `cut` (a host present on the machine cuts or skips content, and its
    documentation or source proves it — Codex past 32 KiB, Claude Code past 4 MiB, Goose past
    its import budget), `secret` (a credential in clear in an instruction file).
  - **2 review**: `over-limit` (past a documented limit whose consequence is not documented —
    Devin 12,000 / 6,000 characters), `ignored` (a file this host never reads), `dead-import`
    (an `@path` that brings nothing in), `frontmatter` (a rule that does not apply as written).
  - **3 good to know**: `shadowed` (another file takes its place), `over-length` (past the
    host's recommendation or near its budget), `duplicate` (the same text loaded twice),
    `comments` (HTML comments sent to the model), `legacy` (an older name still read).
  - **4 information**: `empty`, `unreadable`.
- `linstruct` (findings), `linstruct stacks` (what each host loads, with its budget),
  `linstruct hosts` (presence and sources), `linstruct report` (`bbl-report/1`, hosts present
  on the machine only), `--json`, `--home`, `--host`, `--all-hosts`, `-p <dir>`.
- Started from the Blackberrylabs module template; `bbl-hostrules` 0.4.0 for the locations,
  the readers and the secret masking.
