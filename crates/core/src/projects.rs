//! Where the projects come from (decision L5): the folders the user remembered in
//! `~/.config/linstruct/settings.json` (`project_dirs`, written by the app or the CLI's
//! `folders` command — the only file Linstruct writes), plus the projects Claude Code knows
//! (`~/.claude.json`). A remembered folder is a project itself when it carries a marker, else
//! each of its direct subfolders that does. Read-only here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bbl_hostrules::env::Env;
use bbl_hostrules::resolve::project_roots;
use bbl_hostrules::rules::Scope;
use serde_json::Value;

use crate::rules::{Role, Sheets};

/// `~/.config/linstruct/settings.json`, honouring `XDG_CONFIG_HOME`.
pub fn settings_path(env: &Env) -> PathBuf {
    env.vars
        .get("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| env.home.join(".config"))
        .join("linstruct")
        .join("settings.json")
}

/// The folders remembered in the settings file (`project_dirs`), as written.
pub fn saved_project_dirs(env: &Env) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(settings_path(env)) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    v.get("project_dirs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter_map(|s| env.expand(s))
                .collect()
        })
        .unwrap_or_default()
}

/// What makes a folder a project: a `.git`, or any file a host reads at the project root
/// (taken from the sheets: `CLAUDE.md`, `AGENTS.md`, `.cursor/rules/`, `.github/…`).
pub fn markers(sheets: &Sheets) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    out.insert(".git".into());
    for host in &sheets.hosts {
        for loc in &host.locations {
            if loc.scope != Scope::Project || matches!(loc.ext.role, Role::Ignored | Role::Config) {
                continue;
            }
            for spec in loc.path.any.iter().flat_map(|o| o.iter()) {
                // up to the first glob: `.cursor/rules/**/*.mdc` → `.cursor/rules`
                let fixed = match spec.find('*') {
                    Some(i) => spec[..i].trim_end_matches('/'),
                    None => spec,
                };
                if !fixed.is_empty() {
                    out.insert(fixed.to_string());
                }
            }
        }
    }
    out.into_iter().collect()
}

fn is_project(dir: &Path, markers: &[String]) -> bool {
    markers.iter().any(|m| dir.join(m).exists())
}

/// The projects under the remembered folders: a folder with a marker is a project; else
/// its direct subfolders with a marker are.
pub fn projects_in(dirs: &[PathBuf], home: &Path, sheets: &Sheets) -> Vec<PathBuf> {
    let markers = markers(sheets);
    let mut out: BTreeSet<PathBuf> = BTreeSet::new();
    for d in dirs {
        if !d.is_dir() {
            continue;
        }
        // the home directory is never a project itself (`~/.claude/CLAUDE.md` is a user file)
        if d != home && is_project(d, &markers) {
            out.insert(d.clone());
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                let hidden = e.file_name().to_string_lossy().starts_with('.');
                if p.is_dir() && !hidden && is_project(&p, &markers) {
                    out.insert(p);
                }
            }
        }
    }
    out.into_iter().collect()
}

/// The default project roots: the remembered folders' projects, plus the projects Claude
/// Code knows. Only directories that exist, sorted, without duplicates.
pub fn default_roots(env: &Env, sheets: &Sheets) -> Vec<PathBuf> {
    let mut roots: BTreeSet<PathBuf> = project_roots(env).into_iter().collect();
    roots.extend(projects_in(&saved_project_dirs(env), &env.home, sheets));
    roots.into_iter().filter(|r| r.is_dir()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bbl_hostrules::env::Os;

    #[test]
    fn markers_come_from_the_sheets() {
        let m = markers(&crate::rules::embedded());
        for want in [
            ".git",
            "CLAUDE.md",
            "AGENTS.md",
            ".cursor/rules",
            ".github/copilot-instructions.md",
            "GEMINI.md",
            ".goosehints",
            ".kiro/steering",
        ] {
            assert!(m.iter().any(|x| x == want), "missing {want} in {m:?}");
        }
        assert!(!m.iter().any(|x| x.contains('*')));
    }

    #[test]
    fn remembered_folders_yield_their_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let work = home.join("work");
        std::fs::create_dir_all(work.join("a/.git")).unwrap();
        std::fs::create_dir_all(work.join("b")).unwrap();
        std::fs::write(work.join("b/AGENTS.md"), "x").unwrap();
        std::fs::create_dir_all(work.join("c")).unwrap(); // nothing: not a project
        std::fs::create_dir_all(work.join(".hidden/.git")).unwrap();
        std::fs::create_dir_all(home.join("solo/.cursor/rules")).unwrap();
        std::fs::create_dir_all(home.join(".config/linstruct")).unwrap();
        std::fs::write(
            home.join(".config/linstruct/settings.json"),
            r#"{"lang":"en","project_dirs":["~/work","~/solo","~/missing"]}"#,
        )
        .unwrap();
        let env = Env::fixture(home, Os::Linux);
        let sheets = crate::rules::embedded();
        assert_eq!(
            saved_project_dirs(&env),
            vec![home.join("work"), home.join("solo"), home.join("missing")]
        );
        let roots = default_roots(&env, &sheets);
        assert_eq!(
            roots,
            vec![home.join("solo"), work.join("a"), work.join("b")]
        );
    }
}
