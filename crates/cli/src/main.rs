//! Linstruct command line: `linstruct [path…]` (the findings), `linstruct stacks` (what each
//! host loads), `linstruct hosts` (the sheets and their sources), `linstruct report` (for
//! the Blackberrylabs control tower, always JSON), `--json` for scripts. Read-only.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use bbl_hostrules::env::Env;
use clap::{Parser, Subcommand};
use linstruct_core::model::{Inventory, SlotState};
use linstruct_core::scan::{display_home, rel_plain};

#[derive(Parser)]
#[command(
    name = "linstruct",
    version,
    about = "Your AI instruction files, as each agent will actually read them. Read-only."
)]
struct Cli {
    /// A project root to look at; repeatable (default: the projects this machine knows,
    /// else the current directory).
    #[arg(short, long = "project", global = true, value_name = "DIR", num_args = 1..)]
    paths: Vec<PathBuf>,
    /// Machine-readable output.
    #[arg(long, global = true)]
    json: bool,
    /// Look at another home directory (a backup, a fixture) instead of yours.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// Only this host (its id: claude-code, codex, cursor…).
    #[arg(long, global = true)]
    host: Option<String>,
    /// Include the hosts that are not on this machine ("what if").
    #[arg(long, global = true)]
    all_hosts: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// The findings, most serious first (default).
    List,
    /// What each host loads, file by file, with its budget.
    Stacks,
    /// The hosts Linstruct knows, their presence, and where their rules come from.
    Hosts,
    /// The inventory as a `bbl-report/1` document for the Blackberrylabs control tower
    /// (always JSON, hosts present on this machine only).
    Report,
    /// The folders Linstruct remembers as places where your projects live
    /// (`~/.config/linstruct/settings.json`, the only file it writes).
    Folders {
        #[command(subcommand)]
        action: Option<FoldersAction>,
    },
}

#[derive(Subcommand)]
enum FoldersAction {
    /// Remember one or more folders.
    Add { dirs: Vec<PathBuf> },
    /// Forget one or more folders (the folders themselves are untouched).
    Remove { dirs: Vec<PathBuf> },
}

fn main() -> Result<()> {
    match run() {
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe) =>
        {
            Ok(())
        }
        r => r,
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut env = match &cli.home {
        Some(h) => Env::fixture(h, bbl_hostrules::env::Os::Linux),
        None => Env::current(),
    };
    if let Some(Command::Folders { action }) = &cli.command {
        return folders(&mut std::io::stdout().lock(), &env, action.as_ref());
    }
    let mut roots: Vec<PathBuf> = cli
        .paths
        .iter()
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .collect();
    if roots.is_empty() && cli.home.is_none() {
        // the remembered folders and the projects Claude Code knows (decision L5); the
        // current directory is the fallback when there is none
        let known =
            linstruct_core::projects::default_roots(&env, &linstruct_core::rules::embedded());
        if known.is_empty() {
            if let Ok(cwd) = std::env::current_dir() {
                roots.push(cwd);
            }
        }
    }
    env = env.with_roots(roots);
    let started = std::time::Instant::now();
    let inv = linstruct_core::scan(&env);
    let duration_ms = started.elapsed().as_millis() as u64;
    let mut out = std::io::stdout().lock();
    match cli.command.unwrap_or(Command::List) {
        Command::Report => {
            out.write_all(
                linstruct_core::report::build(&inv, duration_ms)
                    .to_json()
                    .as_bytes(),
            )?;
        }
        _ if cli.json => writeln!(out, "{}", serde_json::to_string_pretty(&inv)?)?,
        Command::List => list(&mut out, &inv, cli.host.as_deref(), cli.all_hosts)?,
        Command::Stacks => stacks(&mut out, &inv, cli.host.as_deref(), cli.all_hosts)?,
        Command::Hosts => hosts(&mut out, &inv)?,
        Command::Folders { .. } => unreachable!("handled before the scan"),
    }
    Ok(())
}

fn list(out: &mut impl Write, inv: &Inventory, host: Option<&str>, all: bool) -> Result<()> {
    let shown: Vec<&linstruct_core::Finding> = inv
        .findings
        .iter()
        .filter(|f| (all || f.present) && host.is_none_or(|h| f.host == h))
        .collect();
    let present_hosts: Vec<&str> = inv
        .hosts
        .iter()
        .filter(|h| h.present)
        .map(|h| h.name.as_str())
        .collect();
    writeln!(
        out,
        "{} instruction file(s) in {} project(s) · hosts present: {}",
        inv.files.len(),
        inv.projects.len(),
        if present_hosts.is_empty() {
            "none".to_string()
        } else {
            present_hosts.join(", ")
        }
    )?;
    writeln!(out)?;
    if shown.is_empty() {
        writeln!(out, "No finding.")?;
    }
    let mut last_sev = None;
    for f in &shown {
        let sev = f.kind.severity();
        if last_sev != Some(sev) {
            writeln!(out, "── {} ──", sev.name().to_uppercase())?;
            last_sev = Some(sev);
        }
        let what_if = if f.present { "" } else { " (what if)" };
        writeln!(out, "{:<13} {}{}", f.kind.label(), f.title, what_if)?;
        writeln!(out, "{:<13} {}", "", f.detail)?;
        if let Some(h) = &f.hint {
            writeln!(out, "{:<13} → {h}", "")?;
        }
    }
    writeln!(out)?;
    let absent = inv.findings.len() - inv.present_findings().count();
    if absent > 0 && !all {
        writeln!(
            out,
            "{absent} more finding(s) for hosts not on this machine: --all-hosts"
        )?;
    }
    for n in &inv.notes {
        writeln!(out, "note: {n}")?;
    }
    writeln!(
        out,
        "{} finding(s). Linstruct changes nothing: every action above is yours to take.",
        shown.len()
    )?;
    Ok(())
}

fn stacks(out: &mut impl Write, inv: &Inventory, host: Option<&str>, all: bool) -> Result<()> {
    for h in &inv.hosts {
        if !(all || h.present) || host.is_some_and(|x| x != h.id) {
            continue;
        }
        writeln!(
            out,
            "{} — {}",
            h.name,
            if h.present {
                h.presence.clone()
            } else {
                format!("not on this machine ({})", h.presence)
            }
        )?;
        for st in inv
            .stacks
            .iter()
            .filter(|s| s.host == h.id && !s.slots.is_empty())
        {
            let label = match &st.project {
                Some(p) => display_home(p, &inv.home),
                None => "user level".into(),
            };
            let budget = st
                .budget
                .as_ref()
                .map(|b| {
                    format!(
                        " · budget {} / {} {} ({})",
                        b.used, b.amount, b.unit, b.on_overflow
                    )
                })
                .unwrap_or_default();
            writeln!(
                out,
                "  {label}: {} B ≈ {} tokens at launch{budget}",
                st.bytes, st.tokens_est
            )?;
            for s in &st.slots {
                let f = &inv.files[s.file];
                let state = match &s.state {
                    SlotState::Loaded => "loaded".to_string(),
                    SlotState::OnDemand => s.when.to_string(),
                    SlotState::Cut { at_line, .. } => format!("CUT at line {at_line}"),
                    SlotState::Skipped { reason } => format!("skipped: {reason}"),
                    SlotState::Shadowed { by } => format!("shadowed by {by}"),
                    SlotState::Ignored { reason } => format!("ignored: {reason}"),
                };
                writeln!(
                    out,
                    "    {:<10} {:>7} B  {:<34} {}",
                    s.role,
                    f.bytes,
                    rel_plain(&f.path, st.project.as_deref(), &inv.home),
                    state
                )?;
            }
        }
    }
    Ok(())
}

fn hosts(out: &mut impl Write, inv: &Inventory) -> Result<()> {
    let sheets = linstruct_core::rules::embedded();
    for h in &inv.hosts {
        let sheet = sheets.host(&h.id);
        writeln!(
            out,
            "{:<24} {:<10} {}",
            h.name,
            if h.present { "present" } else { "absent" },
            h.presence
        )?;
        if let Some(s) = sheet {
            let mut urls: Vec<String> = s
                .locations
                .iter()
                .filter_map(|l| l.source.url.clone())
                .collect();
            urls.sort();
            urls.dedup();
            for u in urls {
                writeln!(out, "{:<24} {:<10} {u}", "", "")?;
            }
            if let Some(b) = &s.ext.budget {
                writeln!(
                    out,
                    "{:<24} {:<10} budget: {} {} per {} → {}",
                    "", "", b.amount, b.unit, b.scope, b.on_overflow
                )?;
            }
        }
    }
    writeln!(out)?;
    for n in &inv.not_read {
        writeln!(out, "not read: {} — {}", n.name, n.reason)?;
    }
    Ok(())
}

/// `linstruct folders [add|remove <dir>…]`: the remembered folders, in the settings file.
/// Writing is limited to the `project_dirs` key; the rest of the file is kept as it is.
fn folders(out: &mut impl Write, env: &Env, action: Option<&FoldersAction>) -> Result<()> {
    let path = linstruct_core::projects::settings_path(env);
    let mut settings: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let mut dirs: Vec<String> = settings
        .get("project_dirs")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let shown = |d: &Path| -> String {
        let abs = std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf());
        display_home(&abs, &env.home)
    };
    let changed = match action {
        None => false,
        Some(FoldersAction::Add { dirs: add }) => {
            let mut changed = false;
            for d in add {
                if !d.is_dir() {
                    writeln!(out, "not a directory, skipped: {}", d.display())?;
                    continue;
                }
                let s = shown(d);
                if !dirs.contains(&s) {
                    dirs.push(s);
                    changed = true;
                }
            }
            changed
        }
        Some(FoldersAction::Remove { dirs: rm }) => {
            let before = dirs.len();
            let targets: Vec<String> = rm.iter().map(|d| shown(d)).collect();
            dirs.retain(|d| !targets.contains(d) && !rm.iter().any(|r| r.to_string_lossy() == *d));
            before != dirs.len()
        }
    };
    if changed {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        settings["project_dirs"] = serde_json::Value::Array(
            dirs.iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect(),
        );
        std::fs::write(&path, serde_json::to_string_pretty(&settings)? + "\n")?;
        writeln!(out, "saved to {}", display_home(&path, &env.home))?;
    }
    if dirs.is_empty() {
        writeln!(out, "No folder remembered. `linstruct folders add <dir>` — each subfolder with a .git or an instruction file becomes a project.")?;
    } else {
        writeln!(
            out,
            "Remembered folders ({}):",
            display_home(&path, &env.home)
        )?;
        for d in &dirs {
            writeln!(out, "  {d}")?;
        }
    }
    let roots = linstruct_core::projects::default_roots(env, &linstruct_core::rules::embedded());
    writeln!(
        out,
        "{} project(s) found, with those Claude Code knows (~/.claude.json).",
        roots.len()
    )?;
    Ok(())
}
