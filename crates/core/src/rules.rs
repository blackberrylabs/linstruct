//! Linstruct's host sheets (`hosts/*.toml`): the common schema comes from `bbl-hostrules`;
//! this module adds what a linter of instruction files needs — per location, how the host
//! treats the file (role, when it loads, references, front matter); per host, its budget,
//! its recommended length, how it is detected. Schema documented in `docs/hosts.md`.
//! Every claim in a sheet names its source; the loader refuses a location without one.

use bbl_hostrules::rules::{Format, Location, Scope, Source};
use bbl_hostrules::{Extension, HostExtension, HostRule, Rules};
use serde::{Deserialize, Serialize};

/// What the file is to the host (`kind` in the sheet — `role` is a common field of
/// `bbl-hostrules` locations).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    /// The main instruction file of a level (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`).
    Main,
    /// A personal, uncommitted companion (`CLAUDE.local.md`, `QWEN.local.md`).
    Local,
    /// One rule among several in a rules directory (`.cursor/rules/*.mdc`, `.claude/rules/**/*.md`).
    Rules,
    /// A file that replaces the main one when present (`AGENTS.override.md`).
    Override,
    /// A configuration file that lists instruction paths (`opencode.json`, `kilo.jsonc`,
    /// `.aider.conf.yml`, Codex `config.toml`).
    Config,
    /// A file users write for this host that the host never reads.
    Ignored,
}

/// When the host loads the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Loaded {
    /// At launch, from the project root (the working directory Linstruct assumes).
    Launch,
    /// Only when the host reads or edits a file in that directory.
    OnDemand,
    /// Only when the host is started in that directory (Codex walks root → cwd).
    WhenCwd,
    /// Only when the rule's front matter matches (globs, description) or on mention.
    Conditional,
}

/// How `@path`-style references are expanded by the host for this location.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Imports {
    /// The marker (`@`, `#[[file:`).
    pub syntax: String,
    /// Maximum nesting; 0 = references are expanded but nesting is not documented.
    #[serde(default)]
    pub depth: u32,
    /// `file` (relative to the importing file) or `project` (relative to the project root).
    #[serde(default = "default_relative_to")]
    pub relative_to: String,
    /// A quoted path (`@"a b.md"`) is not imported (Claude Code).
    #[serde(default)]
    pub quoted_not_imported: bool,
    /// References must stay inside the project (Copilot CLI, Goose).
    #[serde(default)]
    pub within_project: bool,
    /// Above this file size the host does not parse references at all (Goose: 128 KiB).
    pub off_above_bytes: Option<usize>,
}

fn default_relative_to() -> String {
    "file".into()
}

fn default_names_mode() -> String {
    "fallback".into()
}

/// What the host reads in a YAML front matter, for this location.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FrontMatter {
    /// Fields the host reads; any other field is ignored.
    #[serde(default)]
    pub fields: Vec<String>,
    /// The file is not loaded at all without a front matter (Cursor `.mdc`).
    #[serde(default)]
    pub required: bool,
    /// Fields that hold globs to check against the project's files.
    #[serde(default)]
    pub glob_fields: Vec<String>,
    /// The rule is applied only on manual mention when none of these fields is set
    /// (Cursor: `alwaysApply`, `globs`, `description`).
    #[serde(default)]
    pub manual_when_none_of: Vec<String>,
    /// A field whose empty list means the rule never applies (Cline `paths: []`).
    pub never_when_empty: Option<String>,
    /// Must start at byte 0 (Kiro).
    #[serde(default)]
    pub at_byte_zero: bool,
    /// The rule loads at launch whatever else is set when one of these holds (`field` set,
    /// or `field=value`): Cursor `alwaysApply=true`, Devin `trigger=always_on`.
    #[serde(default)]
    pub always_when: Vec<String>,
    /// The rule loads only on condition when one of these holds: Claude `paths`, Cursor
    /// `globs` / `description`, Devin `trigger=glob`, Kiro `inclusion=fileMatch`.
    #[serde(default)]
    pub conditional_when: Vec<String>,
    /// What happens on invalid YAML: `everywhere` (rule loads without conditions, Claude
    /// Code) or `not-loaded` (Cursor). Empty: not documented.
    #[serde(default)]
    pub on_invalid: String,
}

/// For `role = "config"`: where the list of instruction paths sits in the file.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ConfigKeys {
    /// Key path of an array of paths / globs / URLs (`instructions`, `read`).
    pub list_key: Option<String>,
    /// Key path of the host's byte budget override (Codex `project_doc_max_bytes`).
    pub budget_key: Option<String>,
    /// Key path of extra accepted file names (Codex `project_doc_fallback_filenames`,
    /// Gemini `context.fileName`); a string or a list of strings.
    pub fallback_key: Option<String>,
    /// `fallback` (used only when no main file loaded, Codex) or `additional` (loaded next
    /// to the defaults, Gemini). Default `fallback`.
    #[serde(default = "default_names_mode")]
    pub names_mode: String,
    /// Paths in the list are relative to: `file` (the config file's directory) or `project`.
    #[serde(default = "default_relative_to")]
    pub relative_to: String,
}

/// The fields Linstruct adds to every location.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstrLocation {
    #[serde(rename = "kind")]
    pub role: Role,
    #[serde(default = "default_loaded")]
    pub loaded: Loaded,
    /// Position in the host's stack; lower loads first. Ties keep sheet order.
    #[serde(default)]
    pub order: u32,
    /// Also looked for in every subdirectory of the project, with this load state.
    pub nested: Option<Loaded>,
    pub imports: Option<Imports>,
    pub frontmatter: Option<FrontMatter>,
    pub config: Option<ConfigKeys>,
    /// Read only when none of these files exists (same directory, relative names, or `~`
    /// paths): `AGENTS.md` for Claude Code when there is no `CLAUDE.md`, `AGENTS.md` for
    /// Codex when there is no `AGENTS.override.md`, `CLAUDE.md` for Kilo when there is no
    /// `AGENTS.md`.
    #[serde(default)]
    pub only_if_absent: Vec<String>,
    /// Look for `only_if_absent` names in the ancestors too, up to the project root
    /// (Claude Code: "no CLAUDE.md in your working directory or above it").
    #[serde(default)]
    pub only_if_absent_in_ancestors: bool,
    /// The file name must match case exactly (Warp, Kilo): a differently-cased file is ignored.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Counted in the host's budget (Codex: the user-level file is not).
    #[serde(default = "default_true")]
    pub counts_in_budget: bool,
    /// The host strips HTML comments before sending the file.
    #[serde(default)]
    pub strips_html_comments: bool,
    /// The newer name, for a `legacy` location.
    pub replaced_by: Option<String>,
    /// Hard per-file size above which the host skips the file (Claude Code: 4 MiB).
    pub max_file_bytes: Option<usize>,
    /// Documented per-file character limit with undocumented consequence (Devin).
    pub max_file_chars: Option<usize>,
    /// Maximum directory depth under a rules directory (Trae: 3).
    pub max_depth: Option<u32>,
    /// A directory with this name is normal (`.clinerules/`): skipped without a finding.
    #[serde(default)]
    pub dir_ok: bool,
    /// Why an `ignored` location is ignored, shown in the finding.
    pub note: Option<String>,
}

fn default_loaded() -> Loaded {
    Loaded::Launch
}

fn default_true() -> bool {
    true
}

impl Extension for InstrLocation {
    fn validate(loc: &Location<Self>) -> Result<(), String> {
        match loc.ext.role {
            Role::Config => {
                if loc.format == Format::Markdown {
                    return Err("a config location needs a structured format".into());
                }
                if loc.ext.config.as_ref().is_none_or(|c| {
                    c.list_key.is_none() && c.budget_key.is_none() && c.fallback_key.is_none()
                }) {
                    return Err(
                        "a config location needs `config.list_key`, `budget_key` or `fallback_key`"
                            .into(),
                    );
                }
            }
            Role::Ignored => {
                if loc.ext.note.is_none() {
                    return Err("an ignored location needs a `note` saying why".into());
                }
            }
            _ => {
                if loc.format != Format::Markdown {
                    return Err(format!(
                        "role `{:?}` is prose: format must be `markdown`",
                        loc.ext.role
                    ));
                }
            }
        }
        if loc.legacy && loc.ext.replaced_by.is_none() {
            return Err("a legacy location needs `replaced_by`".into());
        }
        if loc.ext.nested.is_some() && loc.scope != Scope::Project {
            return Err("`nested` only makes sense for a project location".into());
        }
        Ok(())
    }
}

/// A byte or character budget, with what happens past it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Budget {
    pub amount: usize,
    /// `bytes` or `chars`.
    pub unit: String,
    /// `total` (all project files together) or `file`.
    pub scope: String,
    /// `cut` (the boundary file is truncated, the rest skipped), `skip` (the file is
    /// dropped), `undocumented`.
    pub on_overflow: String,
    /// Levels counted: `project` (default) or `all`.
    #[serde(default)]
    pub note: Option<String>,
    pub source: Source,
}

/// A warning threshold the host applies (Claude Code: 40,000 characters, observed).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Warn {
    pub amount: usize,
    pub unit: String,
    pub source: Source,
}

/// A recommended length from the host's documentation.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Recommended {
    pub lines: usize,
    pub source: Source,
}

/// How presence on the machine is decided.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Detect {
    /// Directories or files whose existence means the host is installed (`~/.codex`).
    #[serde(default)]
    pub paths: Vec<String>,
    /// Binaries looked up on `PATH`.
    #[serde(default)]
    pub binaries: Vec<String>,
}

/// The fields Linstruct adds to every host.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstrHost {
    #[serde(default)]
    pub detect: Detect,
    pub budget: Option<Budget>,
    pub warn: Option<Warn>,
    pub recommended: Option<Recommended>,
    /// The host deduplicates identical files (Copilot CLI, Junie).
    #[serde(default)]
    pub dedup_identical: bool,
    /// The host drops empty files (Codex, Gemini, Roo, Zed).
    #[serde(default)]
    pub skips_empty: bool,
    /// How the host orders and joins the files, in one sourced sentence.
    pub composition: Option<Sourced>,
    /// The command or view where the host shows what it loaded.
    pub shows_loaded: Option<String>,
}

/// A sentence with its source.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sourced {
    pub text: String,
    pub source: Source,
}

impl HostExtension for InstrHost {
    fn validate<X: Extension>(rule: &HostRule<X, Self>) -> Result<(), String> {
        if rule.status == bbl_hostrules::rules::Status::Active
            && rule.ext.detect.paths.is_empty()
            && rule.ext.detect.binaries.is_empty()
        {
            return Err("an active host needs `detect.paths` or `detect.binaries`".into());
        }
        if let Some(b) = &rule.ext.budget {
            if !matches!(b.unit.as_str(), "bytes" | "chars") {
                return Err(format!("budget.unit `{}`: bytes or chars", b.unit));
            }
            if !matches!(b.scope.as_str(), "total" | "file") {
                return Err(format!("budget.scope `{}`: total or file", b.scope));
            }
            if !matches!(b.on_overflow.as_str(), "cut" | "skip" | "undocumented") {
                return Err(format!(
                    "budget.on_overflow `{}`: cut, skip or undocumented",
                    b.on_overflow
                ));
            }
        }
        Ok(())
    }
}

pub type HostSheet = HostRule<InstrLocation, InstrHost>;
pub type Sheets = Rules<InstrLocation, InstrHost>;

/// The sheets embedded in the binary, one per host.
pub const EMBEDDED: &[(&str, &str)] = &[
    (
        "_not-read.toml",
        include_str!("../../../hosts/_not-read.toml"),
    ),
    ("aider.toml", include_str!("../../../hosts/aider.toml")),
    ("amp.toml", include_str!("../../../hosts/amp.toml")),
    (
        "claude-code.toml",
        include_str!("../../../hosts/claude-code.toml"),
    ),
    ("cline.toml", include_str!("../../../hosts/cline.toml")),
    ("codex.toml", include_str!("../../../hosts/codex.toml")),
    ("copilot.toml", include_str!("../../../hosts/copilot.toml")),
    ("cursor.toml", include_str!("../../../hosts/cursor.toml")),
    (
        "devin-desktop.toml",
        include_str!("../../../hosts/devin-desktop.toml"),
    ),
    (
        "gemini-cli.toml",
        include_str!("../../../hosts/gemini-cli.toml"),
    ),
    ("goose.toml", include_str!("../../../hosts/goose.toml")),
    (
        "jetbrains-ai.toml",
        include_str!("../../../hosts/jetbrains-ai.toml"),
    ),
    ("junie.toml", include_str!("../../../hosts/junie.toml")),
    (
        "kilo-code.toml",
        include_str!("../../../hosts/kilo-code.toml"),
    ),
    ("kiro.toml", include_str!("../../../hosts/kiro.toml")),
    (
        "opencode.toml",
        include_str!("../../../hosts/opencode.toml"),
    ),
    (
        "qwen-code.toml",
        include_str!("../../../hosts/qwen-code.toml"),
    ),
    (
        "roo-code.toml",
        include_str!("../../../hosts/roo-code.toml"),
    ),
    ("trae.toml", include_str!("../../../hosts/trae.toml")),
    ("warp.toml", include_str!("../../../hosts/warp.toml")),
    ("zed.toml", include_str!("../../../hosts/zed.toml")),
];

/// The embedded sheets, validated. A sheet that does not load is a bug in the binary.
pub fn embedded() -> Sheets {
    Sheets::load(EMBEDDED.iter().copied()).expect("the embedded host sheets are valid")
}
