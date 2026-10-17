//! The read. For every host sheet and every project root, rebuild the stack of instruction
//! files the host would load — in its order, references expanded, budget simulated — and
//! say what it would cut, ignore or shadow. Read-only; secrets are masked the moment a
//! line is looked at; nothing is written anywhere.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use bbl_hostrules::env::Env;
use bbl_hostrules::read::{get, read_tree};
use bbl_hostrules::resolve::{expand_glob, resolve};
use bbl_hostrules::rules::{Scope, Source};
use serde_json::Value;

use crate::model::{
    tokens_est, BudgetUse, Finding, HostStatus, InstrFile, Inventory, Kind, Slot, SlotState, Stack,
};
use crate::rules::{self, HostSheet, InstrLocation, Loaded, Role, Sheets};
use crate::text;

/// Directories never walked for nested files or glob checks.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "vendor",
    ".next",
    ".cache",
    "__pycache__",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    "coverage",
];
const MAX_WALK_DEPTH: usize = 8;
const MAX_PROJECT_FILES: usize = 50_000;
const MIN_PARAGRAPH: usize = 60;

/// Scan with the embedded sheets. Project roots: the environment's when given, else the
/// remembered folders and the projects the machine knows (`projects::default_roots`).
pub fn scan(env: &Env) -> Inventory {
    scan_with(env, &rules::embedded())
}

pub fn scan_with(env: &Env, sheets: &Sheets) -> Inventory {
    // explicit roots (`-p`, the app's arguments) are taken as they are; otherwise the
    // remembered folders and the projects Claude Code knows (decision L5)
    let mut roots: Vec<PathBuf> = if env.roots.is_empty() {
        crate::projects::default_roots(env, sheets)
    } else {
        env.roots.clone()
    };
    roots.retain(|r| r.is_dir());
    roots.sort();
    roots.dedup();
    let mut s = Scanner::new(env, sheets, roots);
    s.run();
    s.finish()
}

struct Candidate<'a> {
    loc_index: usize,
    loc: &'a bbl_hostrules::rules::Location<InstrLocation>,
    path: PathBuf,
    /// Directory the file was looked for in (project root, a nested directory, or the
    /// user directory), for `only_if_absent` and depth.
    dir: PathBuf,
    when: Loaded,
}

struct Scanner<'a> {
    env: &'a Env,
    sheets: &'a Sheets,
    roots: Vec<PathBuf>,
    files: Vec<InstrFile>,
    texts: Vec<Option<String>>,
    index: HashMap<PathBuf, usize>,
    findings: Vec<Finding>,
    stacks: Vec<Stack>,
    notes: Vec<String>,
    errors: Vec<String>,
    dirs_cache: HashMap<PathBuf, Vec<PathBuf>>,
    files_cache: HashMap<PathBuf, Vec<String>>,
    /// Per host, from its config file: budget override and extra accepted names.
    budget_override: HashMap<String, usize>,
    fallback_names: HashMap<String, Vec<String>>,
    /// Per host, from its config file: names loaded next to the defaults.
    extra_names: HashMap<String, Vec<String>>,
    file_level_done: HashSet<(Kind, PathBuf)>,
    host_status: Vec<HostStatus>,
}

impl<'a> Scanner<'a> {
    fn new(env: &'a Env, sheets: &'a Sheets, roots: Vec<PathBuf>) -> Self {
        Scanner {
            env,
            sheets,
            roots,
            files: Vec::new(),
            texts: Vec::new(),
            index: HashMap::new(),
            findings: Vec::new(),
            stacks: Vec::new(),
            notes: Vec::new(),
            errors: Vec::new(),
            dirs_cache: HashMap::new(),
            files_cache: HashMap::new(),
            budget_override: HashMap::new(),
            fallback_names: HashMap::new(),
            extra_names: HashMap::new(),
            file_level_done: HashSet::new(),
            host_status: Vec::new(),
        }
    }

    fn run(&mut self) {
        let hosts: Vec<&HostSheet> = self.sheets.hosts.iter().collect();
        for host in hosts {
            let (present, presence) = self.presence(host);
            let before = self.findings.len();
            let mut files_in_host: HashSet<usize> = HashSet::new();
            let mut tokens = 0usize;
            // user level first: a config file there may change the project stacks
            let user = self.build_stack(host, present, None);
            tokens += user.tokens_est;
            files_in_host.extend(
                user.slots
                    .iter()
                    .filter(|s| counts(&s.state))
                    .map(|s| s.file),
            );
            self.stacks.push(user);
            let roots = self.roots.clone();
            let user_slots = self
                .stacks
                .last()
                .map(|s| s.slots.clone())
                .unwrap_or_default();
            for root in &roots {
                let st = self.build_stack(host, present, Some(root));
                tokens += st.tokens_est;
                files_in_host.extend(st.slots.iter().filter(|s| counts(&s.state)).map(|s| s.file));
                // the same text at the user level and in the project is paid twice
                let mut both = user_slots.clone();
                both.extend(st.slots.iter().cloned());
                self.check_duplicates(host, present, Some(root), &both);
                self.stacks.push(st);
            }
            if roots.is_empty() {
                let user_slots = self
                    .stacks
                    .last()
                    .map(|s| s.slots.clone())
                    .unwrap_or_default();
                self.check_duplicates(host, present, None, &user_slots);
            }
            for &f in &files_in_host {
                if !self.files[f].hosts.contains(&host.id) {
                    self.files[f].hosts.push(host.id.clone());
                }
            }
            self.host_status.push(HostStatus {
                id: host.id.clone(),
                name: host.name.clone(),
                present,
                presence,
                files: files_in_host.len(),
                tokens_est: tokens,
                findings: self.findings[before..].len(),
            });
        }
    }

    fn finish(mut self) -> Inventory {
        let order: HashMap<&str, usize> = self
            .sheets
            .hosts
            .iter()
            .enumerate()
            .map(|(i, h)| (h.id.as_str(), i))
            .collect();
        self.findings.sort_by(|a, b| {
            b.present
                .cmp(&a.present)
                .then(a.kind.severity().cmp(&b.kind.severity()))
                .then(order.get(a.host.as_str()).cmp(&order.get(b.host.as_str())))
                .then_with(|| a.title.cmp(&b.title))
                .then_with(|| a.id.cmp(&b.id))
        });
        self.findings.dedup_by(|a, b| a.id == b.id);
        Inventory {
            home: self.env.home.clone(),
            projects: self.roots,
            files: self.files,
            stacks: self.stacks,
            hosts: self.host_status,
            findings: self.findings,
            not_read: self.sheets.not_read.clone(),
            notes: self.notes,
            errors: self.errors,
        }
    }

    // ---- presence -------------------------------------------------------------------

    fn presence(&self, host: &HostSheet) -> (bool, String) {
        for p in &host.ext.detect.paths {
            if let Some(path) = self.env.expand(p) {
                if let Some(found) = expand_glob(&path).into_iter().find(|x| x.exists()) {
                    return (
                        true,
                        format!("{} exists", display_home(&found, &self.env.home)),
                    );
                }
            }
        }
        for b in &host.ext.detect.binaries {
            if let Some(path) = self.env.which(b) {
                return (true, format!("`{}` on PATH ({})", b, path.display()));
            }
        }
        let mut looked: Vec<String> = host.ext.detect.paths.clone();
        looked.extend(host.ext.detect.binaries.iter().map(|b| format!("`{b}`")));
        (false, format!("not found: {}", looked.join(", ")))
    }

    // ---- files -----------------------------------------------------------------------

    /// Read a file once; the index into `files`.
    fn register(&mut self, path: &Path, project: Option<&Path>) -> usize {
        if let Some(&i) = self.index.get(path) {
            return i;
        }
        let (text, error) = match std::fs::read(path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(t) => (Some(t), None),
                Err(_) => (None, Some("not UTF-8 text".to_string())),
            },
            Err(e) => (None, Some(e.to_string())),
        };
        let (bytes, chars, lines, empty) = match &text {
            Some(t) => (
                t.len(),
                t.chars().count(),
                t.lines().count(),
                t.trim().is_empty(),
            ),
            None => (0, 0, 0, false),
        };
        self.files.push(InstrFile {
            path: path.to_path_buf(),
            project: project.map(Path::to_path_buf),
            bytes,
            chars,
            lines,
            tokens_est: tokens_est(bytes),
            empty,
            error,
            hosts: Vec::new(),
        });
        self.texts.push(text);
        let i = self.files.len() - 1;
        self.index.insert(path.to_path_buf(), i);
        i
    }

    fn subdirs(&mut self, root: &Path) -> Vec<PathBuf> {
        if let Some(v) = self.dirs_cache.get(root) {
            return v.clone();
        }
        let mut out = Vec::new();
        walk_dirs(root, 0, &mut out);
        out.sort();
        self.dirs_cache.insert(root.to_path_buf(), out.clone());
        out
    }

    /// Relative paths of the project's files (forward slashes), for glob checks.
    fn project_files(&mut self, root: &Path) -> Vec<String> {
        if let Some(v) = self.files_cache.get(root) {
            return v.clone();
        }
        let mut out = Vec::new();
        walk_files(root, root, 0, &mut out);
        self.files_cache.insert(root.to_path_buf(), out.clone());
        out
    }

    // ---- one stack -------------------------------------------------------------------

    fn build_stack(&mut self, host: &'a HostSheet, present: bool, project: Option<&Path>) -> Stack {
        let candidates = self.candidates(host, present, project);
        let mut slots: Vec<Slot> = Vec::new();
        let mut slot_paths: HashSet<PathBuf> = HashSet::new();
        for c in &candidates {
            if slot_paths.contains(&c.path) {
                continue;
            }
            let before = slots.len();
            self.process_candidate(host, present, project, c, &mut slots, 0);
            for s in &slots[before..] {
                slot_paths.insert(self.files[s.file].path.clone());
            }
        }
        // Additional names (Gemini `context.fileName`): loaded at the project root like the defaults.
        if let (Some(root), Some(names)) = (project, self.extra_names.get(&host.id).cloned()) {
            for n in names {
                let p = root.join(&n);
                if p.is_file() && !slot_paths.contains(&p) {
                    slot_paths.insert(p.clone());
                    let f = self.register(&p, Some(root));
                    slots.push(Slot {
                        file: f,
                        role: "configured".into(),
                        scope: "project".into(),
                        when: "launch".into(),
                        state: SlotState::Loaded,
                        bytes_loaded: self.files[f].bytes,
                        imported_by: None,
                    });
                    self.check_prose(host, present, project, f, None, &mut slots);
                }
            }
        }
        // Fallback names (Codex `project_doc_fallback_filenames`): when nothing main loaded
        // in the project root, try them there.
        if let (Some(root), Some(names)) = (project, self.fallback_names.get(&host.id).cloned()) {
            let has_main = slots.iter().any(|s| {
                s.role == "main"
                    && matches!(s.state, SlotState::Loaded)
                    && self.files[s.file].path.parent() == Some(root)
            });
            if !has_main {
                for n in names {
                    let p = root.join(&n);
                    if p.is_file() && !slot_paths.contains(&p) {
                        let f = self.register(&p, Some(root));
                        slots.push(Slot {
                            file: f,
                            role: "fallback".into(),
                            scope: "project".into(),
                            when: "launch".into(),
                            state: SlotState::Loaded,
                            bytes_loaded: self.files[f].bytes,
                            imported_by: None,
                        });
                        self.check_prose(host, present, project, f, None, &mut slots);
                        break;
                    }
                }
            }
        }
        let budget = self.apply_budget(host, present, project, &mut slots);
        self.check_lengths(host, present, project, &slots);
        let (mut bytes, mut chars, mut lines) = (0, 0, 0);
        for s in &slots {
            if matches!(s.state, SlotState::Loaded | SlotState::Cut { .. }) {
                let f = &self.files[s.file];
                bytes += s.bytes_loaded;
                if s.bytes_loaded == f.bytes {
                    chars += f.chars;
                    lines += f.lines;
                } else if let Some(t) = &self.texts[s.file] {
                    let part = &t[..floor_char(t, s.bytes_loaded)];
                    chars += part.chars().count();
                    lines += part.lines().count();
                }
            }
        }
        Stack {
            host: host.id.clone(),
            project: project.map(Path::to_path_buf),
            slots,
            bytes,
            chars,
            lines,
            tokens_est: tokens_est(bytes),
            budget,
        }
    }

    fn candidates(
        &mut self,
        host: &'a HostSheet,
        present: bool,
        project: Option<&Path>,
    ) -> Vec<Candidate<'a>> {
        let mut out: Vec<Candidate<'a>> = Vec::new();
        for (i, loc) in host.locations.iter().enumerate() {
            let project_scope = loc.scope == Scope::Project;
            if project_scope != project.is_some() {
                continue;
            }
            let roots: Vec<PathBuf> = project.map(|p| vec![p.to_path_buf()]).unwrap_or_default();
            for r in resolve(loc, self.env, &roots) {
                let dir = match project {
                    Some(p) => p.to_path_buf(),
                    None => r.path.parent().map(Path::to_path_buf).unwrap_or_default(),
                };
                out.push(Candidate {
                    loc_index: i,
                    loc,
                    path: r.path,
                    dir,
                    when: loc.ext.loaded,
                });
            }
            if let (Some(root), Some(when)) = (project, loc.ext.nested) {
                let specs: Vec<String> = spec_strings(loc, self.env);
                for d in self.subdirs(root) {
                    for spec in &specs {
                        for p in expand_glob(&d.join(spec)) {
                            if p.exists() {
                                out.push(Candidate {
                                    loc_index: i,
                                    loc,
                                    path: p,
                                    dir: d.clone(),
                                    when,
                                });
                            }
                        }
                    }
                }
            }
            // differently-cased names the host ignores
            if loc.ext.case_sensitive {
                let specs: Vec<String> = spec_strings(loc, self.env);
                let mut dirs = vec![];
                if let Some(root) = project {
                    dirs.push(root.to_path_buf());
                    if loc.ext.nested.is_some() {
                        dirs.extend(self.subdirs(root));
                    }
                }
                for d in dirs {
                    for spec in &specs {
                        let want = Path::new(spec)
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if want.contains('*') {
                            continue;
                        }
                        if let Ok(rd) = std::fs::read_dir(&d) {
                            for e in rd.flatten() {
                                let name = e.file_name().to_string_lossy().to_string();
                                if name != want
                                    && name.eq_ignore_ascii_case(&want)
                                    && e.path().is_file()
                                {
                                    let path = e.path();
                                    let f = self.register(&path, project);
                                    self.finding(host, present, project, Kind::Ignored, Some(f), None, format!("{} does not read `{}`: the name must be `{}`", host.name, name, want), format!("The file name must match case exactly for {}; `{}` is not read.", host.name, name), Some(format!("Rename `{name}` to `{want}`")), Some(&loc.source), Some(name.to_lowercase()));
                                }
                            }
                        }
                    }
                }
            }
        }
        // host order, then sheet order, then path
        out.sort_by(|a, b| {
            a.loc
                .ext
                .order
                .cmp(&b.loc.ext.order)
                .then(a.loc_index.cmp(&b.loc_index))
                .then_with(|| a.path.cmp(&b.path))
        });
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn process_candidate(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        c: &Candidate,
        slots: &mut Vec<Slot>,
        _depth: u32,
    ) {
        let loc = c.loc;
        let path = &c.path;
        if path.is_dir() {
            if loc.ext.role != Role::Config && loc.ext.role != Role::Ignored && !loc.ext.dir_ok {
                let f = self.register_dir(path, project);
                self.finding(
                    host,
                    present,
                    project,
                    Kind::Unreadable,
                    Some(f),
                    None,
                    format!("`{}` is a directory, not a file", file_name(path)),
                    "A directory with the name of an instruction file is skipped by the host."
                        .into(),
                    Some("Rename the directory or put the instructions in a file".into()),
                    Some(&loc.source),
                    None,
                );
            }
            return;
        }
        if !path.is_file() {
            return;
        }
        let scope = scope_name(loc.scope);
        let when = when_name(c.when);
        match loc.ext.role {
            Role::Ignored => {
                let name = file_name(path);
                let configured = self
                    .extra_names
                    .get(&host.id)
                    .into_iter()
                    .chain(self.fallback_names.get(&host.id))
                    .any(|v| v.iter().any(|n| n == &name));
                if configured {
                    return;
                }
                let f = self.register(path, project);
                self.finding(
                    host,
                    present,
                    project,
                    Kind::Ignored,
                    Some(f),
                    None,
                    format!(
                        "{} does not read `{}`",
                        host.name,
                        rel(path, project, &self.env.home)
                    ),
                    loc.ext.note.clone().unwrap_or_default(),
                    loc.ext
                        .replaced_by
                        .as_ref()
                        .map(|r| format!("Move the instructions to `{r}`")),
                    Some(&loc.source),
                    None,
                );
                slots.push(Slot {
                    file: f,
                    role: "ignored".into(),
                    scope,
                    when,
                    state: SlotState::Ignored {
                        reason: loc.ext.note.clone().unwrap_or_default(),
                    },
                    bytes_loaded: 0,
                    imported_by: None,
                });
                return;
            }
            Role::Config => {
                self.process_config(host, present, project, c, slots);
                return;
            }
            _ => {}
        }
        let f = self.register(path, project);
        // shadowed?
        if let Some(by) = self.shadowed_by(c, project) {
            self.finding(
                host,
                present,
                project,
                Kind::Shadowed,
                Some(f),
                None,
                format!(
                    "`{}` is not read by {}: `{}` takes its place",
                    rel(path, project, &self.env.home),
                    host.name,
                    by
                ),
                format!(
                    "{} reads `{}` only when there is no `{}` here.{}",
                    host.name,
                    file_name(path),
                    by,
                    loc.ext
                        .note
                        .as_ref()
                        .map(|n| format!(" {n}"))
                        .unwrap_or_default()
                ),
                Some(format!(
                    "Merge the instructions into `{by}`, or remove the one you do not use"
                )),
                Some(&loc.source),
                None,
            );
            slots.push(Slot {
                file: f,
                role: role_name(loc.ext.role),
                scope,
                when,
                state: SlotState::Shadowed { by },
                bytes_loaded: 0,
                imported_by: None,
            });
            return;
        }
        // depth under a rules directory
        if let Some(max) = loc.ext.max_depth {
            let depth = path
                .strip_prefix(&c.dir)
                .ok()
                .map(|r| r.components().count().saturating_sub(1))
                .unwrap_or(0) as u32;
            // depth counted from the rules directory itself: `.trae/rules/a/b/c/x.md` → 3
            let under_rules = rules_dir_depth(path, &c.dir);
            if under_rules.unwrap_or(depth) > max {
                self.finding(host, present, project, Kind::Ignored, Some(f), None, format!("`{}` is nested deeper than {} reads", rel(path, project, &self.env.home), host.name), format!("{} reads at most {} levels of folders under its rules directory; this file is deeper.", host.name, max), Some("Move the rule up to a shallower folder".into()), Some(&loc.source), None);
                slots.push(Slot {
                    file: f,
                    role: role_name(loc.ext.role),
                    scope,
                    when,
                    state: SlotState::Ignored {
                        reason: "too deep".into(),
                    },
                    bytes_loaded: 0,
                    imported_by: None,
                });
                return;
            }
        }
        if loc.legacy {
            self.finding(
                host,
                present,
                project,
                Kind::Legacy,
                Some(f),
                None,
                format!(
                    "`{}` is a legacy name for {}",
                    rel(path, project, &self.env.home),
                    host.name
                ),
                format!(
                    "{} still reads it, but the documented name is now `{}`.",
                    host.name,
                    loc.ext.replaced_by.clone().unwrap_or_default()
                ),
                Some(format!(
                    "Move the instructions to `{}`",
                    loc.ext.replaced_by.clone().unwrap_or_default()
                )),
                Some(&loc.source),
                None,
            );
        }
        let file = self.files[f].clone();
        if let Some(e) = &file.error {
            self.file_once(
                host,
                present,
                project,
                Kind::Unreadable,
                f,
                None,
                format!("`{}` cannot be read", rel(path, project, &self.env.home)),
                e.clone(),
                None,
                Some(&loc.source),
            );
            slots.push(Slot {
                file: f,
                role: role_name(loc.ext.role),
                scope,
                when,
                state: SlotState::Skipped { reason: e.clone() },
                bytes_loaded: 0,
                imported_by: None,
            });
            return;
        }
        if file.empty {
            self.file_once(
                host,
                present,
                project,
                Kind::Empty,
                f,
                None,
                format!("`{}` is empty", rel(path, project, &self.env.home)),
                if host.ext.skips_empty {
                    format!("{} skips an empty file.", host.name)
                } else {
                    format!("{} loads it for nothing.", host.name)
                },
                Some("Write the instructions, or delete the file".into()),
                Some(&loc.source),
            );
            let state = if host.ext.skips_empty {
                SlotState::Skipped {
                    reason: "empty".into(),
                }
            } else if c.when == Loaded::Launch {
                SlotState::Loaded
            } else {
                SlotState::OnDemand
            };
            slots.push(Slot {
                file: f,
                role: role_name(loc.ext.role),
                scope,
                when,
                state,
                bytes_loaded: 0,
                imported_by: None,
            });
            return;
        }
        // per-file hard limits
        if let Some(max) = loc.ext.max_file_bytes {
            if file.bytes > max {
                self.finding(host, present, project, Kind::Cut, Some(f), None, format!("{} skips `{}`: {} over {}", host.name, rel(path, project, &self.env.home), human_bytes(file.bytes), human_bytes(max)), format!("{} loads a file of up to {} in full and skips a larger file. Nothing in it is read.", host.name, human_bytes(max)), Some("Split the file; move reference material out of the instructions".into()), Some(&loc.source), None);
                slots.push(Slot {
                    file: f,
                    role: role_name(loc.ext.role),
                    scope,
                    when,
                    state: SlotState::Skipped {
                        reason: format!("over {}", human_bytes(max)),
                    },
                    bytes_loaded: 0,
                    imported_by: None,
                });
                return;
            }
        }
        if let Some(max) = loc.ext.max_file_chars {
            if file.chars > max {
                self.finding(host, present, project, Kind::OverLimit, Some(f), None, format!("`{}` has {} characters; {} limits it to {}", rel(path, project, &self.env.home), file.chars, host.name, max), format!("The documentation states the limit; what {} does past it is not documented.", host.name), Some(format!("Split the file so each part stays under {max} characters")), Some(&loc.source), None);
            }
        }
        // front matter
        let mut state = match c.when {
            Loaded::Launch => SlotState::Loaded,
            _ => SlotState::OnDemand,
        };
        let mut when_label = when.clone();
        if let Some(fm) = &loc.ext.frontmatter {
            if let Some((st, w)) = self.check_front_matter(host, present, project, loc, f, fm) {
                state = st;
                when_label = w;
            }
        }
        let loaded_bytes = if matches!(state, SlotState::Loaded | SlotState::OnDemand) {
            file.bytes
        } else {
            0
        };
        slots.push(Slot {
            file: f,
            role: role_name(loc.ext.role),
            scope: scope.clone(),
            when: when_label,
            state: state.clone(),
            bytes_loaded: loaded_bytes,
            imported_by: None,
        });
        if !matches!(state, SlotState::Loaded | SlotState::OnDemand) {
            return;
        }
        self.check_prose(host, present, project, f, Some(loc), slots);
    }

    /// The checks on a file's text that need the file loaded: secrets, comments, references.
    fn check_prose(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        f: usize,
        loc: Option<&bbl_hostrules::rules::Location<InstrLocation>>,
        slots: &mut Vec<Slot>,
    ) {
        let Some(text) = self.texts[f].clone() else {
            return;
        };
        let path = self.files[f].path.clone();
        let source = loc.map(|l| &l.source);
        // secrets — once per file, whoever loads it
        for (i, line) in text.lines().enumerate() {
            for (masked, why) in text::secrets_on_line(line) {
                let key = (Kind::Secret, path.join(format!("#{}", i + 1)));
                if self.file_level_done.insert(key) {
                    self.finding(host, present, project, Kind::Secret, Some(f), Some(i + 1), format!("A secret in `{}` line {}", rel(&path, project, &self.env.home), i + 1), format!("{why}: `{masked}`. The file is sent with every request of every host that loads it."), Some("Move the value to an environment variable or a secret manager and reference it (`${NAME}`)".into()), source, Some(format!("line {}", i + 1)));
                }
            }
        }
        // HTML comments
        if let Some(l) = loc {
            if !l.ext.strips_html_comments {
                let comments = text::html_comments(&text);
                if !comments.is_empty() {
                    let total: usize = comments.iter().map(|c| c.1).sum();
                    self.finding(host, present, project, Kind::Comments, Some(f), Some(comments[0].0), format!("{} sends the {} HTML comment(s) of `{}` to the model", host.name, comments.len(), rel(&path, project, &self.env.home)), format!("{} bytes of `<!-- … -->` go into the prompt with the rest; Claude Code strips such comments, {} does not.", total, host.name), Some("Keep maintainer notes outside the file, or accept that the model reads them".into()), source, None);
                }
            }
        }
        // references
        let imports = loc.and_then(|l| l.ext.imports.clone());
        let within = loc
            .map(|l| {
                l.ext
                    .imports
                    .as_ref()
                    .map(|i| i.within_project)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let _ = within;
        match imports {
            Some(imp) => self.expand_imports(host, present, project, f, &imp, 1, slots, source),
            None => {
                // a host that does not expand references: an `@path` to a real file is a
                // reference the author meant and the model reads literally
                let role_prose = loc
                    .map(|l| {
                        matches!(
                            l.ext.role,
                            Role::Main | Role::Local | Role::Rules | Role::Override
                        )
                    })
                    .unwrap_or(true);
                if role_prose {
                    for r in text::references(&text, "@") {
                        if let Some(target) = self.resolve_ref(&r.target, &path, project, "file") {
                            if target.is_file() {
                                self.finding(host, present, project, Kind::DeadImport, Some(f), Some(r.line), format!("{} does not expand `@{}` in `{}`", host.name, r.target, rel(&path, project, &self.env.home)), format!("The file exists, but {} does not inline `@` references: the model reads the literal `@{}` and may or may not open it.", host.name, r.target), Some("Paste the content, or point to the file in plain words (\"see docs/style.md\")".into()), source, Some(format!("line {}/{}", r.line, r.target)));
                            }
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_imports(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        f: usize,
        imp: &rules::Imports,
        depth: u32,
        slots: &mut Vec<Slot>,
        source: Option<&Source>,
    ) {
        let Some(text) = self.texts[f].clone() else {
            return;
        };
        let path = self.files[f].path.clone();
        let refs = text::references(&text, &imp.syntax);
        if let Some(max) = imp.off_above_bytes {
            if self.files[f].bytes > max && !refs.is_empty() {
                let shown = rel(&path, project, &self.env.home);
                self.finding(host, present, project, Kind::Cut, Some(f), Some(refs[0].line), format!("{} does not expand the {} reference(s) of `{}`: file over {}", host.name, refs.len(), shown, human_bytes(max)), format!("Above {} the host skips reference parsing for the whole file; the text stays, the imported files are not brought in.", human_bytes(max)), Some("Split the file so each part stays under the limit".into()), source, Some("refs-off".into()));
                return;
            }
        }
        for r in refs {
            let shown = rel(&path, project, &self.env.home);
            if r.quoted && imp.quoted_not_imported {
                self.finding(
                    host,
                    present,
                    project,
                    Kind::DeadImport,
                    Some(f),
                    Some(r.line),
                    format!(
                        "`@\"{}\"` in `{}` is not imported: quoted path",
                        r.target, shown
                    ),
                    format!("{} does not import a path wrapped in quotes.", host.name),
                    Some("Escape spaces with a backslash instead of quoting".into()),
                    source,
                    Some(format!("line {}/{}", r.line, r.target)),
                );
                continue;
            }
            let Some(target) = self.resolve_ref(&r.target, &path, project, &imp.relative_to) else {
                continue;
            };
            if imp.within_project {
                if let Some(root) = project {
                    if !target.starts_with(root) {
                        self.finding(
                            host,
                            present,
                            project,
                            Kind::DeadImport,
                            Some(f),
                            Some(r.line),
                            format!("`@{}` in `{}` points outside the project", r.target, shown),
                            format!(
                                "{} only loads references that stay within the repository.",
                                host.name
                            ),
                            Some("Copy the file into the project, or drop the reference".into()),
                            source,
                            Some(format!("line {}/{}", r.line, r.target)),
                        );
                        continue;
                    }
                }
            }
            if target.is_dir() {
                continue;
            }
            if !target.is_file() {
                self.finding(
                    host,
                    present,
                    project,
                    Kind::DeadImport,
                    Some(f),
                    Some(r.line),
                    format!("`@{}` in `{}` points to a missing file", r.target, shown),
                    format!(
                        "{} would import it at launch; there is nothing at `{}`.",
                        host.name,
                        rel(&target, project, &self.env.home)
                    ),
                    Some("Fix the path or remove the reference".into()),
                    source,
                    Some(format!("line {}/{}", r.line, r.target)),
                );
                continue;
            }
            if imp.depth > 0 && depth > imp.depth {
                self.finding(
                    host,
                    present,
                    project,
                    Kind::DeadImport,
                    Some(f),
                    Some(r.line),
                    format!(
                        "`@{}` in `{}` is {} levels deep; {} stops at {}",
                        r.target, shown, depth, host.name, imp.depth
                    ),
                    format!(
                        "Imports nest at most {} levels for {}; this one is not loaded.",
                        imp.depth, host.name
                    ),
                    Some("Flatten the chain of imports".into()),
                    source,
                    Some(format!("line {}/{}", r.line, r.target)),
                );
                continue;
            }
            if slots.iter().any(|s| self.files[s.file].path == target) {
                continue;
            }
            let t = self.register(&target, project);
            let bytes = self.files[t].bytes;
            slots.push(Slot {
                file: t,
                role: "import".into(),
                scope: "import".into(),
                when: "launch".into(),
                state: SlotState::Loaded,
                bytes_loaded: bytes,
                imported_by: Some(f),
            });
            // the imported file is prose too
            self.check_prose(host, present, project, t, None, slots);
            self.expand_imports(host, present, project, t, imp, depth + 1, slots, source);
        }
    }

    fn resolve_ref(
        &self,
        target: &str,
        from: &Path,
        project: Option<&Path>,
        relative_to: &str,
    ) -> Option<PathBuf> {
        if target.starts_with("http://") || target.starts_with("https://") {
            return None;
        }
        if let Some(rest) = target.strip_prefix("~/") {
            return Some(self.env.home.join(rest));
        }
        let p = Path::new(target);
        if p.is_absolute() {
            return Some(p.to_path_buf());
        }
        let base = match (relative_to, project) {
            ("project", Some(root)) => root.to_path_buf(),
            _ => from.parent().map(Path::to_path_buf).unwrap_or_default(),
        };
        Some(base.join(p))
    }

    fn shadowed_by(&self, c: &Candidate, project: Option<&Path>) -> Option<String> {
        let exists_other = |p: &Path| expand_glob(p).iter().any(|x| x.exists() && x != &c.path);
        for pat in &c.loc.ext.only_if_absent {
            if let Some(rest) = pat.strip_prefix("~/") {
                if exists_other(&self.env.home.join(rest)) {
                    return Some(pat.clone());
                }
                continue;
            }
            if c.loc.ext.only_if_absent_in_ancestors {
                // "in your working directory or above it": the project root (the working
                // directory Linstruct assumes) and its ancestors, up to the home directory
                let start = project.unwrap_or(c.dir.as_path());
                let mut dir = Some(start);
                while let Some(d) = dir {
                    let p = d.join(pat);
                    if exists_other(&p) {
                        return Some(rel(&p, project, &self.env.home));
                    }
                    if d == self.env.home || d.parent().is_none() {
                        break;
                    }
                    dir = d.parent();
                }
            } else {
                // the candidate's own directory
                let p = c.dir.join(pat);
                if exists_other(&p) {
                    return Some(rel(&p, project, &self.env.home));
                }
            }
        }
        None
    }

    fn check_front_matter(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        loc: &bbl_hostrules::rules::Location<InstrLocation>,
        f: usize,
        fm: &rules::FrontMatter,
    ) -> Option<(SlotState, String)> {
        let text = self.texts[f].clone()?;
        let path = self.files[f].path.clone();
        let shown = rel(&path, project, &self.env.home);
        let src = Some(&loc.source);
        let parsed = text::front_matter(&text);
        let Some(parsed) = parsed else {
            if fm.required {
                self.finding(host, present, project, Kind::Frontmatter, Some(f), Some(1), format!("`{}` has no front matter: {} does not load it", shown, host.name), format!("{} needs a `---` header (`alwaysApply`, `globs`, `description`) to load a rule; without it the rule is silently skipped.", host.name), Some("Add a front matter with `alwaysApply: true`, or `globs`, or a `description`".into()), src, None);
                return Some((
                    SlotState::Ignored {
                        reason: "no front matter".into(),
                    },
                    "never".into(),
                ));
            }
            return None;
        };
        let block = match parsed {
            Ok(b) => b,
            Err(e) => {
                return Some(
                    self.invalid_front_matter(host, present, project, loc, f, fm, &shown, e, 1),
                );
            }
        };
        if fm.at_byte_zero && block.offset > 0 {
            self.finding(host, present, project, Kind::Frontmatter, Some(f), Some(block.line), format!("The front matter of `{}` does not start the file", shown), format!("{} reads the inclusion settings only when `---` is the very first content; here {} byte(s) come before it, so the file is treated as always included.", host.name, block.offset), Some("Remove the blank lines before `---`".into()), src, None);
        }
        let map = match text::parse_front_matter(&block.yaml) {
            Ok(m) => m,
            Err(e) => {
                return Some(self.invalid_front_matter(
                    host, present, project, loc, f, fm, &shown, e, block.line,
                ))
            }
        };
        // ignored fields
        if !fm.fields.is_empty() {
            let unknown: Vec<&String> = map.keys().filter(|k| !fm.fields.contains(k)).collect();
            if !unknown.is_empty() {
                let list: Vec<String> = unknown.iter().map(|k| format!("`{k}`")).collect();
                self.finding(host, present, project, Kind::Frontmatter, Some(f), Some(block.line), format!("{} ignores {} in the front matter of `{}`", host.name, list.join(", "), shown), format!("{} reads only {}; any other field is ignored without an error, so the rule does not behave as the header suggests.", host.name, fm.fields.iter().map(|x| format!("`{x}`")).collect::<Vec<_>>().join(", ")), Some(format!("Keep only {}", fm.fields.iter().map(|x| format!("`{x}`")).collect::<Vec<_>>().join(", "))), src, Some("fields".into()));
            }
        }
        // never when empty
        if let Some(k) = &fm.never_when_empty {
            if let Some(Value::Array(a)) = map.get(k) {
                if a.is_empty() {
                    self.finding(
                        host,
                        present,
                        project,
                        Kind::Frontmatter,
                        Some(f),
                        Some(block.line),
                        format!("`{k}: []` in `{}`: the rule never applies", shown),
                        format!(
                            "For {}, an empty `{k}` list means the rule never activates.",
                            host.name
                        ),
                        Some(format!(
                            "Remove `{k}` to make the rule always active, or list patterns"
                        )),
                        src,
                        Some("never".into()),
                    );
                    return Some((
                        SlotState::Ignored {
                            reason: format!("{k}: []"),
                        },
                        "never".into(),
                    ));
                }
            }
        }
        // globs that match nothing
        if let Some(root) = project {
            let files = self.project_files(root);
            for gf in &fm.glob_fields {
                if let Some(v) = map.get(gf) {
                    let globs = text::field_list(v);
                    let dead: Vec<String> = globs
                        .iter()
                        .filter(|g| !glob_matches_any(g, &files))
                        .cloned()
                        .collect();
                    if !globs.is_empty() && dead.len() == globs.len() {
                        self.finding(host, present, project, Kind::Frontmatter, Some(f), Some(block.line), format!("No file of the project matches `{gf}` in `{}`", shown), format!("Pattern(s) {}: nothing in this project matches, so the rule never attaches.", dead.iter().map(|d| format!("`{d}`")).collect::<Vec<_>>().join(", ")), Some("Fix the pattern (`**/*.ts` for any depth) or drop it".into()), src, Some(gf.clone()));
                    }
                }
            }
        }
        // conditional / always / manual
        let always = fm.always_when.iter().any(|w| cond_set(&map, w));
        let conditional = fm.conditional_when.iter().any(|w| cond_set(&map, w));
        if always {
            return Some((SlotState::Loaded, "launch".into()));
        }
        if !fm.manual_when_none_of.is_empty()
            && !fm.manual_when_none_of.iter().any(|w| cond_set(&map, w))
        {
            self.finding(host, present, project, Kind::Frontmatter, Some(f), Some(block.line), format!("`{}` applies only when mentioned by hand", shown), format!("None of {} is set, so {} includes the rule only when you @-mention it in chat.", fm.manual_when_none_of.iter().map(|x| format!("`{x}`")).collect::<Vec<_>>().join(", "), host.name), Some("Set `alwaysApply: true`, or `globs`, or a `description`".into()), src, Some("manual".into()));
            return Some((SlotState::OnDemand, "manual".into()));
        }
        if conditional {
            return Some((SlotState::OnDemand, "conditional".into()));
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn invalid_front_matter(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        loc: &bbl_hostrules::rules::Location<InstrLocation>,
        f: usize,
        fm: &rules::FrontMatter,
        shown: &str,
        e: String,
        line: usize,
    ) -> (SlotState, String) {
        let (detail, state) = match fm.on_invalid.as_str() {
            "everywhere" => (format!("{} ignores the broken header and loads the rule as if it had no conditions: it applies everywhere.", host.name), (SlotState::Loaded, "launch".to_string())),
            "not-loaded" => (format!("{} does not load a rule whose front matter is broken.", host.name), (SlotState::Ignored { reason: "invalid front matter".into() }, "never".to_string())),
            _ => (format!("What {} does with a broken front matter is not documented.", host.name), (SlotState::Loaded, "launch".to_string())),
        };
        self.finding(
            host,
            present,
            project,
            Kind::Frontmatter,
            Some(f),
            Some(line),
            format!("Invalid front matter in `{}`", shown),
            format!("{e}. {detail}"),
            Some("Fix the YAML (quote values with `:` or `*`, close the `---`)".into()),
            Some(&loc.source),
            Some("invalid".into()),
        );
        state
    }

    fn process_config(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        c: &Candidate,
        slots: &mut Vec<Slot>,
    ) {
        let loc = c.loc;
        let Some(cfg) = &loc.ext.config else { return };
        let (_, tree) = match read_tree(&c.path, loc.format) {
            Ok(t) => t,
            Err(e) => {
                self.errors.push(format!("{}: {e}", c.path.display()));
                return;
            }
        };
        if let Some(k) = &cfg.budget_key {
            if let Some(n) = get(&tree, k).and_then(Value::as_u64) {
                self.budget_override.insert(host.id.clone(), n as usize);
                self.notes.push(format!(
                    "{}: `{k}` = {n} in {}",
                    host.name,
                    display_home(&c.path, &self.env.home)
                ));
            }
        }
        if let Some(k) = &cfg.fallback_key {
            let names: Vec<String> = match get(&tree, k) {
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                Some(Value::String(s)) => vec![s.clone()],
                _ => Vec::new(),
            };
            if !names.is_empty() {
                self.notes.push(format!(
                    "{}: `{k}` = {} in {}",
                    host.name,
                    names.join(", "),
                    display_home(&c.path, &self.env.home)
                ));
                if cfg.names_mode == "additional" {
                    self.extra_names
                        .entry(host.id.clone())
                        .or_default()
                        .extend(names);
                } else {
                    self.fallback_names.insert(host.id.clone(), names);
                }
            }
        }
        if let Some(k) = &cfg.list_key {
            if let Some(Value::Array(a)) = get(&tree, k) {
                let cfg_file = self.register(&c.path, project);
                for v in a.iter().filter_map(Value::as_str) {
                    if v.starts_with("http://") || v.starts_with("https://") {
                        self.notes.push(format!(
                            "{}: {} lists a remote instruction `{}` — never fetched by Linstruct",
                            host.name,
                            display_home(&c.path, &self.env.home),
                            v
                        ));
                        continue;
                    }
                    let base = match (cfg.relative_to.as_str(), project) {
                        ("project", Some(root)) => root.to_path_buf(),
                        _ => c.path.parent().map(Path::to_path_buf).unwrap_or_default(),
                    };
                    let spec = if let Some(rest) = v.strip_prefix("~/") {
                        self.env.home.join(rest)
                    } else if Path::new(v).is_absolute() {
                        PathBuf::from(v)
                    } else {
                        base.join(v)
                    };
                    let found = expand_glob(&spec);
                    let existing: Vec<PathBuf> =
                        found.into_iter().filter(|p| p.is_file()).collect();
                    if existing.is_empty() {
                        self.finding(
                            host,
                            present,
                            project,
                            Kind::DeadImport,
                            Some(cfg_file),
                            None,
                            format!("`{}` listed in {} is missing", v, file_name(&c.path)),
                            format!(
                                "{} would load it as instructions; nothing is at `{}`.",
                                host.name,
                                spec.display()
                            ),
                            Some("Fix the path or remove the entry".into()),
                            Some(&loc.source),
                            Some(v.to_string()),
                        );
                        continue;
                    }
                    for p in existing {
                        if slots.iter().any(|s| self.files[s.file].path == p) {
                            continue;
                        }
                        let f = self.register(&p, project);
                        let bytes = self.files[f].bytes;
                        slots.push(Slot {
                            file: f,
                            role: "listed".into(),
                            scope: scope_name(loc.scope),
                            when: "launch".into(),
                            state: SlotState::Loaded,
                            bytes_loaded: bytes,
                            imported_by: Some(cfg_file),
                        });
                        self.check_prose(host, present, project, f, None, slots);
                    }
                }
            }
        }
    }

    // ---- budget, lengths, duplicates ----------------------------------------------------

    fn apply_budget(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        slots: &mut [Slot],
    ) -> Option<BudgetUse> {
        let b = host.ext.budget.as_ref()?;
        let amount = self
            .budget_override
            .get(&host.id)
            .copied()
            .unwrap_or(b.amount);
        let unit = b.unit.clone();
        let src = Some(&b.source);
        // only project-level stacks count unless the sheet says otherwise
        if project.is_none() && b.note.as_deref() != Some("all") {
            return None;
        }
        let measure = |f: &InstrFile, text: &Option<String>| -> usize {
            if unit == "chars" {
                text.as_ref().map(|t| t.chars().count()).unwrap_or(f.chars)
            } else {
                f.bytes
            }
        };
        let mut used = 0usize;
        if b.scope == "file" {
            for s in slots.iter_mut() {
                if !matches!(s.state, SlotState::Loaded) {
                    continue;
                }
                let size = measure(&self.files[s.file], &self.texts[s.file]);
                used = used.max(size);
                if size > amount {
                    let f = s.file;
                    match b.on_overflow.as_str() {
                        "skip" => {
                            s.state = SlotState::Skipped {
                                reason: format!("over {amount} {unit}"),
                            };
                            s.bytes_loaded = 0;
                            let shown = rel(&self.files[f].path, project, &self.env.home);
                            self.finding(
                                host,
                                present,
                                project,
                                Kind::Cut,
                                Some(f),
                                None,
                                format!(
                                    "{} drops `{}`: {} {} over its {}",
                                    host.name, shown, size, unit, amount
                                ),
                                format!("Past {amount} {unit} the file is dropped entirely."),
                                Some("Split the file".into()),
                                src,
                                None,
                            );
                        }
                        "cut" => {
                            let at = if unit == "chars" {
                                char_to_byte(self.texts[f].as_deref().unwrap_or(""), amount)
                            } else {
                                amount
                            };
                            let line = text::line_at(self.texts[f].as_deref().unwrap_or(""), at);
                            s.state = SlotState::Cut {
                                at_byte: at,
                                at_line: line,
                            };
                            s.bytes_loaded = at;
                            let shown = rel(&self.files[f].path, project, &self.env.home);
                            self.finding(host, present, project, Kind::Cut, Some(f), Some(line), format!("{} cuts `{}` at line {}", host.name, shown, line), format!("The file has {size} {unit}; {} reads the first {amount} and drops the rest without a message.", host.name), Some(format!("Split the file below line {line}, or move reference material out")), src, None);
                        }
                        _ => {
                            let shown = rel(&self.files[f].path, project, &self.env.home);
                            self.finding(host, present, project, Kind::OverLimit, Some(f), None, format!("`{}` has {} {}; {} limits it to {}", shown, size, unit, host.name, amount), "The documentation states the limit; what the host does past it is not documented.".into(), Some("Split the file".into()), src, None);
                        }
                    }
                }
            }
            return Some(BudgetUse {
                amount,
                unit,
                scope: "file".into(),
                used,
                on_overflow: b.on_overflow.clone(),
            });
        }
        // total budget over the launch-loaded files, in stack order
        let mut remaining = amount;
        let mut exhausted = false;
        let mut cut_detail: Option<(usize, usize)> = None;
        let mut skipped_after: Vec<String> = Vec::new();
        let loc_counts: HashMap<usize, bool> = slots.iter().map(|s| (s.file, true)).collect();
        let _ = loc_counts;
        for s in slots.iter_mut() {
            if !matches!(s.state, SlotState::Loaded) {
                continue;
            }
            let loc_counts_in_budget = self.counts_in_budget(host, s.file);
            if !loc_counts_in_budget {
                continue;
            }
            let size = measure(&self.files[s.file], &self.texts[s.file]);
            if exhausted {
                s.state = SlotState::Skipped {
                    reason: "budget exhausted".into(),
                };
                s.bytes_loaded = 0;
                skipped_after.push(rel(&self.files[s.file].path, project, &self.env.home));
                continue;
            }
            if size > remaining {
                match b.on_overflow.as_str() {
                    "cut" => {
                        let text = self.texts[s.file].as_deref().unwrap_or("");
                        let at = if unit == "chars" {
                            char_to_byte(text, remaining)
                        } else {
                            remaining
                        };
                        let line = text::line_at(text, at);
                        s.state = SlotState::Cut {
                            at_byte: at,
                            at_line: line,
                        };
                        s.bytes_loaded = at;
                        used += remaining;
                        cut_detail = Some((s.file, line));
                    }
                    _ => {
                        s.state = SlotState::Skipped {
                            reason: "over budget".into(),
                        };
                        s.bytes_loaded = 0;
                        skipped_after.push(rel(&self.files[s.file].path, project, &self.env.home));
                    }
                }
                exhausted = true;
                remaining = 0;
            } else {
                remaining -= size;
                used += size;
            }
        }
        if let Some((f, line)) = cut_detail {
            let shown = rel(&self.files[f].path, project, &self.env.home);
            let tail = if skipped_after.is_empty() {
                String::new()
            } else {
                format!(
                    " The file(s) after it are skipped entirely: {}.",
                    skipped_after
                        .iter()
                        .map(|s| format!("`{s}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            self.finding(host, present, project, Kind::Cut, Some(f), Some(line), format!("{} cuts `{}` at line {}", host.name, shown, line), format!("{} reads at most {amount} {unit} of project instructions in total; the budget runs out inside this file and the rest is dropped with no message in the interface.{tail}", host.name), Some(format!("Split the instructions so the whole chain stays under {amount} {unit}, or raise the limit in the host's configuration")), src, None);
        } else if exhausted {
            if let Some(first) = skipped_after.first().cloned() {
                let f = slots
                    .iter()
                    .find(|s| matches!(s.state, SlotState::Skipped { .. }))
                    .map(|s| s.file);
                self.finding(
                    host,
                    present,
                    project,
                    Kind::Cut,
                    f,
                    None,
                    format!(
                        "{} drops `{}`: budget of {amount} {unit} exhausted",
                        host.name, first
                    ),
                    format!(
                        "Files after the budget are skipped: {}.",
                        skipped_after
                            .iter()
                            .map(|s| format!("`{s}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Some("Split or shorten the earlier files".into()),
                    src,
                    None,
                );
            }
        } else if amount > 0 && used * 10 >= amount * 8 && used > 0 {
            let f = slots
                .iter()
                .rev()
                .find(|s| matches!(s.state, SlotState::Loaded))
                .map(|s| s.file);
            self.finding(host, present, project, Kind::OverLength, f, None, format!("{}: {} of {} {} used", host.name, used, amount, unit), format!("The project's instructions use {}% of {}'s budget; past it, content is dropped without a message.", used * 100 / amount, host.name), Some("Keep some room, or raise the limit in the host's configuration".into()), src, Some("near-budget".into()));
        }
        // when-cwd chains: the budget again, for each nested file the host would start from
        self.when_cwd_chains(host, present, project, slots, amount, &unit, b);
        Some(BudgetUse {
            amount,
            unit,
            scope: "total".into(),
            used,
            on_overflow: b.on_overflow.clone(),
        })
    }

    fn counts_in_budget(&self, host: &HostSheet, file: usize) -> bool {
        let path = &self.files[file].path;
        for loc in &host.locations {
            if !loc.ext.counts_in_budget {
                for spec in spec_strings(loc, self.env) {
                    if let Some(p) = self.env.expand(&spec) {
                        if &p == path {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn when_cwd_chains(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        slots: &[Slot],
        amount: usize,
        unit: &str,
        b: &rules::Budget,
    ) {
        let Some(root) = project else { return };
        if b.on_overflow != "cut" {
            return;
        }
        let root_loaded: usize = slots
            .iter()
            .filter(|s| {
                matches!(s.state, SlotState::Loaded | SlotState::Cut { .. })
                    && self.counts_in_budget(host, s.file)
            })
            .map(|s| s.bytes_loaded)
            .sum();
        let nested: Vec<&Slot> = slots
            .iter()
            .filter(|s| s.when == "when-cwd" && matches!(s.state, SlotState::OnDemand))
            .collect();
        for s in &nested {
            let dir = self.files[s.file]
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            // chain: root files, then the when-cwd files whose directory is an ancestor-or-self of `dir`, shallowest first
            let mut chain: Vec<&Slot> = nested
                .iter()
                .copied()
                .filter(|o| dir.starts_with(self.files[o.file].path.parent().unwrap_or(root)))
                .collect();
            chain.sort_by_key(|o| self.files[o.file].path.components().count());
            let mut remaining = amount.saturating_sub(root_loaded);
            for o in chain {
                let size = if unit == "chars" {
                    self.files[o.file].chars
                } else {
                    self.files[o.file].bytes
                };
                if size > remaining {
                    let text = self.texts[o.file].as_deref().unwrap_or("");
                    let at = if unit == "chars" {
                        char_to_byte(text, remaining)
                    } else {
                        remaining
                    };
                    let line = text::line_at(text, at);
                    let shown = rel(&self.files[o.file].path, project, &self.env.home);
                    let from = rel(&dir, project, &self.env.home);
                    self.finding(host, present, project, Kind::Cut, Some(o.file), Some(line), format!("{} started in `{}` cuts `{}` at line {}", host.name, from, shown, line), format!("{} builds its chain from the project root down to the directory it starts in; with the root file(s), the {amount} {unit} run out inside this file.", host.name), Some("Shorten the root file or this one".into()), Some(&b.source), Some(format!("from {from}")));
                    break;
                }
                remaining -= size;
                if self.files[o.file].path == self.files[s.file].path {
                    break;
                }
            }
        }
    }

    fn check_lengths(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        slots: &[Slot],
    ) {
        let warn = host.ext.warn.clone();
        let reco = host.ext.recommended.clone();
        if warn.is_none() && reco.is_none() {
            return;
        }
        for s in slots {
            if !matches!(
                s.state,
                SlotState::Loaded | SlotState::OnDemand | SlotState::Cut { .. }
            ) || s.role == "import"
                || s.role == "listed"
            {
                continue;
            }
            let f = &self.files[s.file];
            let shown = rel(&f.path, project, &self.env.home);
            let mut facts: Vec<String> = Vec::new();
            let mut src: Option<Source> = None;
            if let Some(w) = &warn {
                let size = if w.unit == "chars" { f.chars } else { f.bytes };
                if size > w.amount {
                    facts.push(format!(
                        "{} {} where {} warns above {}",
                        size, w.unit, host.name, w.amount
                    ));
                    src = Some(w.source.clone());
                }
            }
            if let Some(r) = &reco {
                if f.lines > r.lines {
                    facts.push(format!(
                        "{} lines where {} recommends under {}",
                        f.lines, host.name, r.lines
                    ));
                    if src.is_none() {
                        src = Some(r.source.clone());
                    }
                }
            }
            if facts.is_empty() {
                continue;
            }
            let file = s.file;
            let more = if facts.len() > 1 {
                format!(" Also: {}.", facts[1..].join("; "))
            } else {
                String::new()
            };
            self.finding(host, present, project, Kind::OverLength, Some(file), None, format!("`{}` is long for {}: {}", shown, host.name, facts[0]), format!("Every line is sent with every request; past the host's guidance, adherence drops.{more}"), Some("Keep what is always true; move reference material to files the agent opens on demand".into()), src.as_ref(), None);
        }
    }

    fn check_duplicates(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        slots: &[Slot],
    ) {
        let mut seen: HashMap<String, (usize, usize)> = HashMap::new();
        let mut pairs: BTreeMap<(usize, usize), (usize, usize, usize)> = BTreeMap::new();
        let loaded: Vec<&Slot> = slots
            .iter()
            .filter(|s| {
                matches!(
                    s.state,
                    SlotState::Loaded | SlotState::OnDemand | SlotState::Cut { .. }
                )
            })
            .collect();
        for s in &loaded {
            let Some(text) = &self.texts[s.file] else {
                continue;
            };
            for (norm, line) in text::paragraphs(text, MIN_PARAGRAPH) {
                match seen.get(&norm) {
                    Some(&(other, other_line)) if other != s.file => {
                        let key = if other < s.file {
                            (other, s.file)
                        } else {
                            (s.file, other)
                        };
                        let e = pairs.entry(key).or_insert((0, other_line, line));
                        e.0 += 1;
                    }
                    Some(_) => {}
                    None => {
                        seen.insert(norm, (s.file, line));
                    }
                }
            }
        }
        for ((a, b), (count, line_a, line_b)) in pairs {
            let fa = &self.files[a];
            let fb = &self.files[b];
            let identical = fa.bytes == fb.bytes && self.texts[a] == self.texts[b];
            if identical && host.ext.dedup_identical {
                continue;
            }
            let sa = rel(&fa.path, project, &self.env.home);
            let sb = rel(&fb.path, project, &self.env.home);
            let (title, detail) = if identical {
                (
                    format!("`{sa}` and `{sb}` are identical for {}", host.name),
                    format!(
                        "{} loads both files; the same {} bytes go into the context twice.",
                        host.name, fa.bytes
                    ),
                )
            } else {
                (format!("{} paragraph(s) of `{sa}` repeated in `{sb}` for {}", count, host.name), format!("{} loads both files; the shared text (first at line {line_a} of `{sa}`, line {line_b} of `{sb}`) is paid twice on every request.", host.name))
            };
            self.finding(
                host,
                present,
                project,
                Kind::Duplicate,
                Some(a),
                Some(line_a),
                title,
                detail,
                Some(
                    "Keep the text in one of the two files; point to it from the other".to_string(),
                ),
                None,
                Some(sb.replace('/', "_")),
            );
        }
    }

    // ---- findings --------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn finding(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        kind: Kind,
        file: Option<usize>,
        line: Option<usize>,
        title: String,
        detail: String,
        hint: Option<String>,
        source: Option<&Source>,
        suffix: Option<String>,
    ) {
        let path = file.map(|f| self.files[f].path.clone());
        let subject = path
            .as_ref()
            .map(|p| rel(p, project, &self.env.home))
            .unwrap_or_else(|| host.name.clone());
        let id_path = match (&path, project) {
            (Some(p), Some(root)) => format!(
                "{}/{}",
                display_home(root, &self.env.home).trim_start_matches("~/"),
                rel_plain(p, project, &self.env.home)
            ),
            (Some(p), None) => display_home(p, &self.env.home)
                .trim_start_matches("~/")
                .to_string(),
            (None, _) => host.name.clone(),
        };
        let mut id = format!("{}/{}/{}", kind.slug(), host.id, id_path.replace(' ', "_"));
        if let Some(s) = suffix {
            id.push('/');
            id.push_str(&s.replace(' ', "_"));
        }
        self.findings.push(Finding {
            kind,
            id,
            host: host.id.clone(),
            host_name: host.name.clone(),
            present,
            title,
            subject,
            detail,
            path,
            line,
            hint,
            source_url: source.and_then(|s| s.url.clone()),
        });
    }

    /// A finding about the file itself (unreadable, empty): once, whoever loads it.
    #[allow(clippy::too_many_arguments)]
    fn file_once(
        &mut self,
        host: &HostSheet,
        present: bool,
        project: Option<&Path>,
        kind: Kind,
        f: usize,
        line: Option<usize>,
        title: String,
        detail: String,
        hint: Option<String>,
        source: Option<&Source>,
    ) {
        let key = (kind, self.files[f].path.clone());
        if self.file_level_done.insert(key) {
            self.finding(
                host,
                present,
                project,
                kind,
                Some(f),
                line,
                title,
                detail,
                hint,
                source,
                None,
            );
        }
    }

    fn register_dir(&mut self, path: &Path, project: Option<&Path>) -> usize {
        if let Some(&i) = self.index.get(path) {
            return i;
        }
        self.files.push(InstrFile {
            path: path.to_path_buf(),
            project: project.map(Path::to_path_buf),
            bytes: 0,
            chars: 0,
            lines: 0,
            tokens_est: 0,
            empty: false,
            error: Some("is a directory".into()),
            hosts: Vec::new(),
        });
        self.texts.push(None);
        let i = self.files.len() - 1;
        self.index.insert(path.to_path_buf(), i);
        i
    }
}

// ---- helpers -------------------------------------------------------------------------------

fn counts(state: &SlotState) -> bool {
    matches!(
        state,
        SlotState::Loaded | SlotState::OnDemand | SlotState::Cut { .. }
    )
}

fn scope_name(s: Scope) -> String {
    match s {
        Scope::User => "user",
        Scope::Project => "project",
        Scope::System => "system",
    }
    .into()
}

fn when_name(l: Loaded) -> String {
    match l {
        Loaded::Launch => "launch",
        Loaded::OnDemand => "on-demand",
        Loaded::WhenCwd => "when-cwd",
        Loaded::Conditional => "conditional",
    }
    .into()
}

fn role_name(r: Role) -> String {
    match r {
        Role::Main => "main",
        Role::Local => "local",
        Role::Rules => "rules",
        Role::Override => "override",
        Role::Config => "config",
        Role::Ignored => "ignored",
    }
    .into()
}

/// The path specs of a location for this OS, as written in the sheet.
fn spec_strings(loc: &bbl_hostrules::rules::Location<InstrLocation>, env: &Env) -> Vec<String> {
    use bbl_hostrules::env::Os;
    let mut v: Vec<String> = Vec::new();
    if let Some(a) = &loc.path.any {
        v.extend(a.iter().map(String::from));
    }
    let os = match env.os {
        Os::Linux => loc.path.linux.as_ref(),
        Os::Macos => loc.path.macos.as_ref(),
        Os::Windows => loc.path.windows.as_ref(),
    };
    if let Some(o) = os {
        v.extend(o.iter().map(String::from));
    }
    v
}

fn walk_dirs(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth >= MAX_WALK_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if !ft.is_dir() || ft.is_symlink() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        out.push(p.clone());
        walk_dirs(&p, depth + 1, out);
    }
}

fn walk_files(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth >= MAX_WALK_DEPTH || out.len() >= MAX_PROJECT_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if ft.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk_files(root, &p, depth + 1, out);
            }
        } else if ft.is_file() {
            if let Ok(r) = p.strip_prefix(root) {
                out.push(r.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

/// Does a front-matter glob match any project file? Lenient: the pattern as written, and
/// with `**/` in front (Cursor's `*.ts` means any TypeScript file).
fn glob_matches_any(pattern: &str, files: &[String]) -> bool {
    let opts = glob::MatchOptions {
        case_sensitive: false,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    let mut pats: Vec<glob::Pattern> = Vec::new();
    let p = pattern.trim().trim_start_matches("./");
    if let Ok(g) = glob::Pattern::new(p) {
        pats.push(g);
    }
    if !p.starts_with("**") {
        if let Ok(g) = glob::Pattern::new(&format!("**/{p}")) {
            pats.push(g);
        }
    }
    if p.ends_with('/') {
        if let Ok(g) = glob::Pattern::new(&format!("{p}**")) {
            pats.push(g);
        }
    }
    files
        .iter()
        .any(|f| pats.iter().any(|g| g.matches_with(f, opts)))
}

/// `field` (set and non-empty) or `field=value` (equal, as string).
fn cond_set(map: &serde_json::Map<String, Value>, cond: &str) -> bool {
    match cond.split_once('=') {
        Some((k, v)) => map
            .get(k)
            .map(|x| text::field_list(x).iter().any(|s| s == v))
            .unwrap_or(false),
        None => text::field_set(map.get(cond)),
    }
}

fn rules_dir_depth(path: &Path, dir: &Path) -> Option<u32> {
    // the rules directory: the first ancestor named `rules` under `dir`
    let r = path.strip_prefix(dir).ok()?;
    let comps: Vec<_> = r
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    let i = comps.iter().position(|c| c == "rules")?;
    // folders between `rules` and the file
    Some((comps.len() - i - 2) as u32)
}

fn floor_char(t: &str, byte: usize) -> usize {
    let mut b = byte.min(t.len());
    while b > 0 && !t.is_char_boundary(b) {
        b -= 1;
    }
    b
}

fn char_to_byte(t: &str, chars: usize) -> usize {
    t.char_indices()
        .nth(chars)
        .map(|(i, _)| i)
        .unwrap_or(t.len())
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| p.display().to_string())
}

/// Project-relative path, else `~/…`, else absolute.
/// `<project name>/<path in the project>` for a project file, else `~/…`, else absolute.
/// The project name keeps a finding readable when several projects are scanned.
pub fn rel(path: &Path, project: Option<&Path>, home: &Path) -> String {
    if let Some(root) = project {
        if let Ok(r) = path.strip_prefix(root) {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| display_home(root, home));
            let s = r.to_string_lossy().replace('\\', "/");
            return if s.is_empty() {
                name
            } else {
                format!("{name}/{s}")
            };
        }
    }
    display_home(path, home)
}

/// The path inside the project, without the project's name (the stacks view, where the
/// project is the heading).
pub fn rel_plain(path: &Path, project: Option<&Path>, home: &Path) -> String {
    if let Some(root) = project {
        if let Ok(r) = path.strip_prefix(root) {
            let s = r.to_string_lossy().replace('\\', "/");
            return if s.is_empty() { ".".into() } else { s };
        }
    }
    display_home(path, home)
}

pub fn display_home(path: &Path, home: &Path) -> String {
    if let Ok(r) = path.strip_prefix(home) {
        return format!("~/{}", r.to_string_lossy().replace('\\', "/"));
    }
    path.to_string_lossy().replace('\\', "/")
}

fn human_bytes(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MiB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}
