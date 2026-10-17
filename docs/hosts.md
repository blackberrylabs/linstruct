# Host sheets — `hosts/*.toml`

One file per host, read by the scan, never written. The common part of the schema comes from
[`bbl-hostrules`](https://github.com/blackberrylabs/bbl-hostrules) (`id`, `name`, `vendor`,
`kind`, `status`, `hints`, `locations` with `scope`, `format`, `path`, `legacy`, `source`);
this page lists what Linstruct adds. **Every location names its source** — documentation,
source code or an observation — with the URL, the date it was read and the words it was read
from; the loader refuses a location without one.

## Host fields

| Field | Meaning |
|---|---|
| `detect.paths`, `detect.binaries` | How presence on the machine is decided: a directory or file that exists (`~/.codex`), a binary on `PATH`. An active host needs one of the two. |
| `budget` | `amount`, `unit` (`bytes` \| `chars`), `scope` (`total` over the project's launch-loaded files \| `file`), `on_overflow` (`cut` — the boundary file is truncated and the rest skipped; `skip` — the file is dropped; `undocumented`), `source`. Codex: 32768 bytes, total, cut. Devin: 12000 chars, file, undocumented. |
| `warn` | A threshold the host warns at (`amount`, `unit`, `source`). Claude Code: 40,000 characters, `observed`. |
| `recommended.lines` | The length the documentation recommends. Claude Code 200, Cursor 500. |
| `dedup_identical` | The host drops identical duplicates (Copilot CLI, Junie): no `duplicate` finding for identical files. |
| `skips_empty` | The host drops empty files (Codex, Gemini, Roo, Zed). |
| `composition` | One sourced sentence on how the host orders and joins the files. |
| `shows_loaded` | Where the host itself shows what it loaded (`/context`, `/memory list`, `/instructions`…). |

## Location fields

| Field | Meaning |
|---|---|
| `kind` | `main` (the instruction file of a level), `local` (uncommitted companion), `rules` (one of several in a rules directory), `override` (replaces the main file when present), `config` (a structured file listing instruction paths or setting the budget), `ignored` (a file users write for this host that the host never reads — needs a `note`). |
| `loaded` | `launch` (default), `on-demand` (when the host touches that directory), `when-cwd` (when the host is started there — Codex), `conditional` (front matter decides). |
| `order` | Position in the host's stack, lower first. |
| `nested` | Also looked for in every subdirectory of the project, with this load state. |
| `imports` | `syntax` (`@` or `#[[file:`), `depth` (0 = not documented), `relative_to` (`file` \| `project`), `quoted_not_imported` (Claude Code), `within_project` (Copilot CLI, Goose), `off_above_bytes` (Goose: 128 KiB). Without `imports`, an `@path` to an existing file is a `dead-import`: the model reads the literal. |
| `frontmatter` | `fields` (read by the host; others → `frontmatter` finding), `required` (Cursor `.mdc`), `glob_fields` (checked against the project's files), `always_when` / `conditional_when` (`field` set, or `field=value`), `manual_when_none_of`, `never_when_empty` (Cline `paths: []`), `at_byte_zero` (Kiro), `on_invalid` (`everywhere` — Claude Code; `not-loaded` — Cursor). |
| `config` | For `kind = "config"`: `list_key` (array of paths / globs / URLs — URLs are listed, never fetched), `budget_key` (Codex `project_doc_max_bytes`), `fallback_key` + `names_mode` (`fallback` — Codex; `additional` — Gemini `context.fileName`), `relative_to`. |
| `only_if_absent` | Read only when none of these (relative names, globs, or `~` paths) exists; `only_if_absent_in_ancestors` looks up to the project root too (Claude Code's AGENTS.md). Produces `shadowed`. |
| `case_sensitive` | A differently-cased file is `ignored` (Warp, Kilo). |
| `counts_in_budget` | Default true; Codex's user-level file is not counted. |
| `strips_html_comments` | The host removes `<!-- -->` before sending (Claude Code, Qwen rules); otherwise `comments`. |
| `replaced_by` | The newer name, for a `legacy` or `ignored` location. |
| `max_file_bytes` | The host skips a larger file (Claude Code 4 MiB) → `cut`. |
| `max_file_chars` | Documented limit, undocumented consequence (Devin global 6,000) → `over-limit`. |
| `max_depth` | Folders allowed under a rules directory (Trae 3) → `ignored` beyond. |
| `dir_ok` | A directory with this name is normal (`.clinerules/`). |
| `note` | Shown in the finding (why ignored; a caveat on the behaviour). |

## Sources

`kind = "documentation"` with the page's URL; `kind = "source-code"` with the file's URL and
the commit in `note`; `kind = "observed"` for something seen on a machine or in an issue
tracker (URL when there is one). `read_on` is the day the source was read. When the
documentation and the source code disagree, the sheet says which one the location follows,
and the finding's detail carries the caveat.
