//! What Linstruct finds, in its own words: the instruction files of a machine, the stack
//! each host would load for each project, and the findings. `Kind::severity` and
//! `Kind::slug` are the tower's contract (see `docs/new-module.md` in the tower).

use std::path::PathBuf;

use bbl_report::Severity;
use serde::Serialize;

/// The kinds of finding. One slug each, one place on the four common severities — the
/// mapping is written in the CHANGELOG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A host present on this machine cuts or skips content, and its documentation or
    /// source code proves it: Codex past its byte budget, Claude Code past 4 MiB, Goose past
    /// its import budget.
    Cut,
    /// A secret in clear in an instruction file — it leaves with every prompt.
    Secret,
    /// Past a documented limit whose consequence is not documented (Devin: 12,000 / 6,000
    /// characters).
    OverLimit,
    /// A file written for a host that this host never reads: `.md` in `.cursor/rules`,
    /// `AGENTS.local.md` for Claude Code, `agents.md` in lower case for Warp…
    Ignored,
    /// An `@path` that does nothing: target missing, depth exceeded, quoted path, or a host
    /// that does not expand references (the model reads the literal `@path`).
    DeadImport,
    /// Front matter that makes the rule apply otherwise than written: missing where
    /// required, invalid YAML, ignored field, manual-only rule, globs matching nothing.
    Frontmatter,
    /// Read by the host in principle, but another file takes its place here.
    Shadowed,
    /// Past the length the host recommends, or near a hard budget.
    OverLength,
    /// The same content loaded twice by the same host.
    Duplicate,
    /// HTML comments that this host sends to the model (Claude Code strips them).
    Comments,
    /// A legacy name the host still reads, with the name that replaces it.
    Legacy,
    /// An empty file.
    Empty,
    /// A file that cannot be read.
    Unreadable,
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::Cut,
        Kind::Secret,
        Kind::OverLimit,
        Kind::Ignored,
        Kind::DeadImport,
        Kind::Frontmatter,
        Kind::Shadowed,
        Kind::OverLength,
        Kind::Duplicate,
        Kind::Comments,
        Kind::Legacy,
        Kind::Empty,
        Kind::Unreadable,
    ];

    /// The slug, as it appears in `--json` and in the tower's report.
    pub fn slug(self) -> &'static str {
        match self {
            Kind::Cut => "cut",
            Kind::Secret => "secret",
            Kind::OverLimit => "over-limit",
            Kind::Ignored => "ignored",
            Kind::DeadImport => "dead-import",
            Kind::Frontmatter => "frontmatter",
            Kind::Shadowed => "shadowed",
            Kind::OverLength => "over-length",
            Kind::Duplicate => "duplicate",
            Kind::Comments => "comments",
            Kind::Legacy => "legacy",
            Kind::Empty => "empty",
            Kind::Unreadable => "unreadable",
        }
    }

    /// Where the kind sits on the four common severities (decision L2: a silent cut is 1).
    pub fn severity(self) -> Severity {
        match self {
            Kind::Cut | Kind::Secret => Severity::ActNow,
            Kind::OverLimit | Kind::Ignored | Kind::DeadImport | Kind::Frontmatter => {
                Severity::Review
            }
            Kind::Shadowed | Kind::OverLength | Kind::Duplicate | Kind::Comments | Kind::Legacy => {
                Severity::GoodToKnow
            }
            Kind::Empty | Kind::Unreadable => Severity::Info,
        }
    }

    /// Label as the CLI prints it.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Cut => "CUT",
            Kind::Secret => "SECRET",
            Kind::OverLimit => "OVER LIMIT",
            Kind::Ignored => "IGNORED",
            Kind::DeadImport => "DEAD IMPORT",
            Kind::Frontmatter => "FRONT MATTER",
            Kind::Shadowed => "SHADOWED",
            Kind::OverLength => "OVER LENGTH",
            Kind::Duplicate => "DUPLICATE",
            Kind::Comments => "COMMENTS",
            Kind::Legacy => "LEGACY",
            Kind::Empty => "EMPTY",
            Kind::Unreadable => "UNREADABLE",
        }
    }

    /// One sentence on what the kind means, for the detail pane.
    pub fn meaning(self) -> &'static str {
        match self {
            Kind::Cut => "Content you wrote is not read: the host cuts or skips it, and nothing tells you.",
            Kind::Secret => "A credential sits in a file that is sent with every request.",
            Kind::OverLimit => "The file is past a limit the host documents; what the host does then is not documented.",
            Kind::Ignored => "The host never reads this file; the instructions in it have no effect there.",
            Kind::DeadImport => "A reference that brings nothing in: the target is missing, too deep, or the host does not expand references.",
            Kind::Frontmatter => "The rule does not apply the way its header says.",
            Kind::Shadowed => "The host reads this kind of file, but another file takes its place here.",
            Kind::OverLength => "Longer than the host recommends; every line costs context on every request and dilutes the rest.",
            Kind::Duplicate => "The same text is loaded twice by this host and paid twice.",
            Kind::Comments => "HTML comments are sent to the model by this host.",
            Kind::Legacy => "An older name the host still reads, replaced by a newer one.",
            Kind::Empty => "The file is empty.",
            Kind::Unreadable => "The file could not be read.",
        }
    }
}

/// How a host sees one file in one stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SlotState {
    /// Loaded at launch, counted in the stack.
    Loaded,
    /// Loaded only when the host touches that directory (or starts there).
    OnDemand,
    /// Cut at this byte: what follows is not read.
    Cut { at_byte: usize, at_line: usize },
    /// Skipped entirely (budget exhausted, file too large).
    Skipped { reason: String },
    /// Another file takes its place.
    Shadowed { by: String },
    /// Never read by this host.
    Ignored { reason: String },
}

/// One file in a host's stack.
#[derive(Debug, Clone, Serialize)]
pub struct Slot {
    /// Index into `Inventory::files`.
    pub file: usize,
    pub role: String,
    pub scope: String,
    /// `launch`, `on-demand`, `when-cwd`, `conditional`, `manual`, `never`.
    pub when: String,
    pub state: SlotState,
    /// Bytes the host actually takes from this file in this stack.
    pub bytes_loaded: usize,
    /// Set for a file brought in by an `@` reference, with the importing file's index.
    pub imported_by: Option<usize>,
}

/// What one host would load for one project — or at the user level when `project` is None.
#[derive(Debug, Clone, Serialize)]
pub struct Stack {
    pub host: String,
    pub project: Option<PathBuf>,
    pub slots: Vec<Slot>,
    pub bytes: usize,
    pub chars: usize,
    pub lines: usize,
    pub tokens_est: usize,
    /// The host's budget when it has one: amount, unit, and what the stack uses of it.
    pub budget: Option<BudgetUse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetUse {
    pub amount: usize,
    pub unit: String,
    pub scope: String,
    pub used: usize,
    pub on_overflow: String,
}

/// One instruction file on the machine, read once.
#[derive(Debug, Clone, Serialize)]
pub struct InstrFile {
    pub path: PathBuf,
    /// Project root for a per-project file.
    pub project: Option<PathBuf>,
    pub bytes: usize,
    pub chars: usize,
    pub lines: usize,
    pub tokens_est: usize,
    pub empty: bool,
    pub error: Option<String>,
    /// Hosts whose stacks hold this file.
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub kind: Kind,
    /// Stable across runs for the same thing.
    pub id: String,
    pub host: String,
    pub host_name: String,
    /// False when the host is not on this machine: shown in the app as "what if", never
    /// in the summary nor in the tower's report (decision L4).
    pub present: bool,
    pub title: String,
    pub subject: String,
    pub detail: String,
    pub path: Option<PathBuf>,
    pub line: Option<usize>,
    /// What the person could do — shown, never run.
    pub hint: Option<String>,
    /// The documentation page the rule cites.
    pub source_url: Option<String>,
}

/// One host as Linstruct sees it.
#[derive(Debug, Clone, Serialize)]
pub struct HostStatus {
    pub id: String,
    pub name: String,
    pub present: bool,
    /// How presence was decided (the directory or binary found), or why not.
    pub presence: String,
    pub files: usize,
    pub tokens_est: usize,
    pub findings: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Inventory {
    pub home: PathBuf,
    pub projects: Vec<PathBuf>,
    pub files: Vec<InstrFile>,
    pub stacks: Vec<Stack>,
    pub hosts: Vec<HostStatus>,
    /// Most serious first; present hosts before absent ones.
    pub findings: Vec<Finding>,
    pub not_read: Vec<bbl_hostrules::rules::NotRead>,
    /// What was read and what was not followed (remote instructions, config overrides).
    pub notes: Vec<String>,
    pub errors: Vec<String>,
}

impl Inventory {
    /// Findings of hosts present on this machine — what the summary and the report count.
    pub fn present_findings(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.present)
    }
}

/// Rough token estimate: four bytes per token, shown as an estimate.
pub fn tokens_est(bytes: usize) -> usize {
    bytes.div_ceil(4)
}
