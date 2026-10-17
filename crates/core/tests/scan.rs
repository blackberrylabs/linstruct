//! The scan on a fixture home: one case per kind of finding, on three hosts present
//! (Claude Code, Codex, Cursor) and the others absent ("what if").

use std::fs;
use std::path::{Path, PathBuf};

use bbl_hostrules::env::{Env, Os};
use linstruct_core::model::{Inventory, Kind};

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

/// A home with three hosts installed and one project.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_path_buf();
    let proj = home.join("proj");
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::create_dir_all(home.join(".cursor")).unwrap();
    write(
        &home.join(".codex/config.toml"),
        "model = \"x\"\nproject_doc_max_bytes = 2000\n",
    );
    let shared = "This paragraph is shared between the user file and the project file, so it is paid twice by Claude Code on every request.";
    write(
        &home.join(".claude/CLAUDE.md"),
        &format!("# Me\n\n{shared}\n"),
    );
    // project CLAUDE.md: long, comments, references, a secret, the shared paragraph
    let mut claude = String::from("# Project\n\n<!-- maintainer note -->\n\nRead @docs/style.md first. Also @docs/missing.md and @\"docs/quoted file.md\".\n\n");
    claude.push_str(shared);
    claude.push_str("\n\nOPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz0123456789\n\n");
    for i in 0..220 {
        claude.push_str(&format!("- rule number {i}: keep it short\n"));
    }
    write(&proj.join("CLAUDE.md"), &claude);
    write(&proj.join("docs/style.md"), "# Style\n\nUse spaces.\n");
    write(&proj.join("src/main.rs"), "fn main() {}\n");
    // AGENTS.md: 3000 bytes, over the Codex budget of 2000 set in config.toml
    let mut agents = String::new();
    for i in 0..100 {
        agents.push_str(&format!("line {i:03} of the agents file, padding it out\n"));
    }
    assert!(agents.len() > 2000);
    write(&proj.join("AGENTS.md"), &agents);
    write(
        &proj.join("sub/AGENTS.md"),
        "# Sub\n\nMore rules for the sub package.\n",
    );
    write(
        &proj.join("AGENTS.local.md"),
        "# local\n\nnever read by Claude Code\n",
    );
    write(
        &proj.join(".cursor/rules/plain.md"),
        "# ignored by cursor\n",
    );
    write(&proj.join(".cursor/rules/nofm.mdc"), "# no front matter\n");
    write(
        &proj.join(".cursor/rules/desc.mdc"),
        "---\ndescription: when touching styles\n---\n# conditional\n",
    );
    write(
        &proj.join(".cursor/rules/zig.mdc"),
        "---\nglobs: *.zig\n---\n# nothing matches\n",
    );
    write(
        &proj.join(".cursor/rules/manual.mdc"),
        "---\nalwaysApply: false\n---\n# manual only\n",
    );
    write(
        &proj.join(".cursor/rules/broken.mdc"),
        "---\nglobs: [*.ts\n---\n# invalid yaml\n",
    );
    write(
        &proj.join(".claude/rules/r1.md"),
        "---\npaths: [\"src/**\"]\nglobs: \"src/**\"\n---\n# rule with an ignored field\n",
    );
    write(&proj.join(".cursorrules"), "legacy cursor rules\n");
    write(&proj.join("empty/AGENTS.md"), "\n\n");
    write(&proj.join("agents.md"), "# lower case, ignored by Warp\n");
    fs::create_dir_all(proj.join("GEMINI.md")).unwrap();
    (tmp, proj)
}

fn run(home: &Path, proj: &Path) -> Inventory {
    let env = Env::fixture(home, Os::Linux).with_roots(vec![proj.to_path_buf()]);
    linstruct_core::scan(&env)
}

fn kinds(inv: &Inventory, host: &str) -> Vec<(Kind, String)> {
    inv.findings
        .iter()
        .filter(|f| f.host == host)
        .map(|f| {
            (
                f.kind,
                f.subject
                    .strip_prefix("proj/")
                    .unwrap_or(&f.subject)
                    .to_string(),
            )
        })
        .collect()
}

#[test]
fn presence_is_decided_by_directories_and_binaries() {
    let (tmp, proj) = fixture();
    let inv = run(tmp.path(), &proj);
    let present: Vec<&str> = inv
        .hosts
        .iter()
        .filter(|h| h.present)
        .map(|h| h.id.as_str())
        .collect();
    assert_eq!(present, ["claude-code", "codex", "cursor"]);
    assert!(inv.hosts.len() >= 19);
}

#[test]
fn claude_code_findings() {
    let (tmp, proj) = fixture();
    let inv = run(tmp.path(), &proj);
    let k = kinds(&inv, "claude-code");
    let has = |kind: Kind, subject: &str| k.iter().any(|(kk, s)| *kk == kind && s == subject);
    assert!(has(Kind::Secret, "CLAUDE.md"), "{k:?}");
    assert!(has(Kind::Shadowed, "AGENTS.md"), "{k:?}");
    assert!(has(Kind::Ignored, "AGENTS.local.md"), "{k:?}");
    assert!(has(Kind::DeadImport, "CLAUDE.md"), "{k:?}");
    assert!(has(Kind::Frontmatter, ".claude/rules/r1.md"), "{k:?}");
    assert!(has(Kind::OverLength, "CLAUDE.md"), "{k:?}");
    assert!(
        has(Kind::Duplicate, "~/.claude/CLAUDE.md") || has(Kind::Duplicate, "CLAUDE.md"),
        "{k:?}"
    );
    assert!(
        !has(Kind::Comments, "CLAUDE.md"),
        "Claude strips comments: {k:?}"
    );
    // dead imports: the missing file and the quoted path, not the existing one
    let dead: Vec<&linstruct_core::Finding> = inv
        .findings
        .iter()
        .filter(|f| f.host == "claude-code" && f.kind == Kind::DeadImport)
        .collect();
    assert_eq!(dead.len(), 2, "{dead:#?}");
    assert!(dead.iter().all(|f| f.present));
}

#[test]
fn codex_cuts_at_the_configured_budget() {
    let (tmp, proj) = fixture();
    let inv = run(tmp.path(), &proj);
    let cut: Vec<&linstruct_core::Finding> = inv
        .findings
        .iter()
        .filter(|f| f.host == "codex" && f.kind == Kind::Cut)
        .collect();
    assert!(!cut.is_empty(), "{:#?}", kinds(&inv, "codex"));
    let root_cut = cut
        .iter()
        .find(|f| f.subject == "proj/AGENTS.md")
        .expect("root AGENTS.md is cut");
    assert!(
        root_cut.title.contains("cuts `proj/AGENTS.md` at line"),
        "{}",
        root_cut.title
    );
    assert!(
        root_cut.line.unwrap() > 40 && root_cut.line.unwrap() < 50,
        "2000 bytes ≈ 44 lines: {:?}",
        root_cut.line
    );
    assert!(inv
        .notes
        .iter()
        .any(|n| n.contains("project_doc_max_bytes")));
    // the stack carries the budget and the cut
    let st = inv
        .stacks
        .iter()
        .find(|s| s.host == "codex" && s.project.is_some())
        .unwrap();
    let b = st.budget.as_ref().unwrap();
    assert_eq!(b.amount, 2000);
    assert_eq!(st.bytes, 2000);
    // Codex never reads CLAUDE.md; the empty nested file is reported once
    assert!(kinds(&inv, "codex")
        .iter()
        .any(|(k, s)| *k == Kind::Ignored && s == "CLAUDE.md"));
    assert!(inv
        .findings
        .iter()
        .any(|f| f.kind == Kind::Empty && f.subject == "proj/empty/AGENTS.md"));
    // Codex sends HTML comments... but it does not read CLAUDE.md, so no comments finding for it
    assert!(!kinds(&inv, "codex")
        .iter()
        .any(|(k, _)| *k == Kind::Comments));
}

#[test]
fn cursor_front_matter_cases() {
    let (tmp, proj) = fixture();
    let inv = run(tmp.path(), &proj);
    let k = kinds(&inv, "cursor");
    let has = |kind: Kind, subject: &str| k.iter().any(|(kk, s)| *kk == kind && s == subject);
    assert!(has(Kind::Ignored, ".cursor/rules/plain.md"), "{k:?}");
    assert!(has(Kind::Frontmatter, ".cursor/rules/nofm.mdc"), "{k:?}");
    assert!(has(Kind::Frontmatter, ".cursor/rules/zig.mdc"), "{k:?}");
    assert!(has(Kind::Frontmatter, ".cursor/rules/manual.mdc"), "{k:?}");
    assert!(has(Kind::Frontmatter, ".cursor/rules/broken.mdc"), "{k:?}");
    assert!(!has(Kind::Frontmatter, ".cursor/rules/desc.mdc"), "{k:?}");
    assert!(has(Kind::Legacy, ".cursorrules"), "{k:?}");
    assert!(
        has(Kind::Comments, "CLAUDE.md"),
        "Cursor sends comments: {k:?}"
    );
    // the conditional rule is on demand in the stack
    let st = inv
        .stacks
        .iter()
        .find(|s| s.host == "cursor" && s.project.is_some())
        .unwrap();
    let desc = st
        .slots
        .iter()
        .find(|s| inv.files[s.file].path.ends_with("desc.mdc"))
        .unwrap();
    assert_eq!(desc.when, "conditional");
}

#[test]
fn absent_hosts_are_what_if_only() {
    let (tmp, proj) = fixture();
    let inv = run(tmp.path(), &proj);
    let warp: Vec<&linstruct_core::Finding> =
        inv.findings.iter().filter(|f| f.host == "warp").collect();
    assert!(
        warp.iter()
            .any(|f| f.kind == Kind::Ignored && f.subject == "proj/agents.md"),
        "{warp:#?}"
    );
    assert!(warp.iter().all(|f| !f.present));
    let gemini: Vec<&linstruct_core::Finding> = inv
        .findings
        .iter()
        .filter(|f| f.host == "gemini-cli")
        .collect();
    assert!(
        gemini
            .iter()
            .any(|f| f.kind == Kind::Unreadable && f.subject == "proj/GEMINI.md"),
        "{gemini:#?}"
    );
    // present findings first, then by severity
    let first_absent = inv.findings.iter().position(|f| !f.present).unwrap();
    assert!(inv.findings[..first_absent].iter().all(|f| f.present));
    let sev: Vec<u8> = inv.findings[..first_absent]
        .iter()
        .map(|f| f.kind.severity().number())
        .collect();
    let mut sorted = sev.clone();
    sorted.sort();
    assert_eq!(sev, sorted);
    // the report leaves the absent hosts out
    let report = linstruct_core::report::build(&inv, 1);
    assert_eq!(report.findings.len(), inv.present_findings().count());
    bbl_report::conformance::check(&report.to_json()).unwrap();
}

#[test]
fn two_projects_with_the_same_file_names_keep_distinct_findings() {
    let (tmp, proj) = fixture();
    let other = tmp.path().join("other");
    fs::create_dir_all(&other).unwrap();
    fs::copy(proj.join("CLAUDE.md"), other.join("CLAUDE.md")).unwrap();
    let env = Env::fixture(tmp.path(), Os::Linux).with_roots(vec![proj.clone(), other.clone()]);
    let inv = linstruct_core::scan(&env);
    let long: Vec<&linstruct_core::Finding> = inv
        .findings
        .iter()
        .filter(|f| f.host == "claude-code" && f.kind == Kind::OverLength)
        .collect();
    assert_eq!(long.len(), 2, "{long:#?}");
    assert!(long.iter().any(|f| f.subject == "proj/CLAUDE.md"));
    assert!(long.iter().any(|f| f.subject == "other/CLAUDE.md"));
    assert_ne!(long[0].id, long[1].id);
}

#[test]
fn ids_are_stable_across_runs() {
    let (tmp, proj) = fixture();
    let a: Vec<String> = run(tmp.path(), &proj)
        .findings
        .iter()
        .map(|f| f.id.clone())
        .collect();
    let b: Vec<String> = run(tmp.path(), &proj)
        .findings
        .iter()
        .map(|f| f.id.clone())
        .collect();
    assert_eq!(a, b);
    let mut dedup = a.clone();
    dedup.dedup();
    assert_eq!(a.len(), dedup.len(), "ids are unique");
}
