//! The `bbl-report/1` view of an inventory, for the Blackberrylabs control tower. Nothing
//! here is new: only what the CLI and the app already show, secrets masked upstream.
//! Findings of hosts absent from the machine (the app's "what if") are left out (L4).

use bbl_report::{
    Action, ActionKind, Evidence, Finding, Module, Open, Report, Subject, SubjectKind, Summary,
    Tone,
};

use crate::model::{Inventory, Kind};

/// Binary name of the app, as packaged.
pub const APP: &str = "linstruct-app";
/// Binary name of the command line, as packaged.
pub const CLI: &str = "linstruct";

pub fn module() -> Module {
    Module {
        id: "linstruct".into(),
        name: "Linstruct".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        verb: "lint".into(),
        tagline: "Your AI instruction files, as each agent will actually read them".into(),
        app: Some(APP.into()),
        cli: Some(CLI.into()),
        home: "https://github.com/blackberrylabs/linstruct".into(),
    }
}

/// Build the report. `duration_ms` is the time the scan took.
pub fn build(inv: &Inventory, duration_ms: u64) -> Report {
    let mut report = Report::new(module());
    report.duration_ms = duration_ms;
    let present: Vec<&crate::model::Finding> = inv.present_findings().collect();
    let n = present.len();
    let hosts_present = inv.hosts.iter().filter(|h| h.present).count();
    let heaviest = inv
        .hosts
        .iter()
        .filter(|h| h.present)
        .max_by_key(|h| h.tokens_est);
    let act_now = present
        .iter()
        .filter(|f| f.kind.severity() == bbl_report::Severity::ActNow)
        .count();
    report.summary = vec![
        Summary {
            key: "files".into(),
            label: "Instruction files".into(),
            value: inv
                .files
                .iter()
                .filter(|f| f.error.is_none())
                .count()
                .to_string(),
            tone: Tone::Neutral,
        },
        Summary {
            key: "hosts".into(),
            label: "Hosts reading them".into(),
            value: hosts_present.to_string(),
            tone: Tone::Neutral,
        },
        Summary {
            key: "heaviest".into(),
            label: "Heaviest host (≈ tokens)".into(),
            value: heaviest
                .map(|h| format!("{} ≈{}", h.name, h.tokens_est))
                .unwrap_or_else(|| "—".into()),
            tone: match heaviest.map(|h| h.tokens_est) {
                Some(t) if t > 20_000 => Tone::Amber,
                _ => Tone::Neutral,
            },
        },
        Summary {
            key: "findings".into(),
            label: "Findings".into(),
            value: n.to_string(),
            tone: if act_now > 0 {
                Tone::Alert
            } else if n == 0 {
                Tone::Good
            } else {
                Tone::Amber
            },
        },
    ];
    for f in present {
        let mut actions = Vec::new();
        if let Some(h) = &f.hint {
            actions.push(Action {
                kind: ActionKind::Hint,
                label: h.clone(),
                target: None,
            });
        }
        if let Some(p) = &f.path {
            let target = match f.line {
                Some(l) => format!("{}:{l}", p.display()),
                None => p.display().to_string(),
            };
            actions.push(Action {
                kind: ActionKind::OpenFile,
                label: "Open file".into(),
                target: Some(target),
            });
        }
        if let Some(u) = &f.source_url {
            if u.starts_with("http") {
                actions.push(Action {
                    kind: ActionKind::OpenUrl,
                    label: "Read the host's documentation".into(),
                    target: Some(u.clone()),
                });
            }
        }
        let subject_kind = if f.path.is_some() {
            SubjectKind::File
        } else {
            SubjectKind::Host
        };
        report.findings.push(Finding {
            id: f.id.clone(),
            severity: f.kind.severity(),
            kind: f.kind.slug().into(),
            title: f.title.clone(),
            subject: Subject {
                kind: subject_kind,
                label: format!("{} · {}", f.host_name, f.subject),
            },
            detail: format!("{} {}", f.detail, f.kind.meaning()),
            evidence: f
                .path
                .iter()
                .map(|p| Evidence {
                    label: f.subject.clone(),
                    path: Some(p.display().to_string()),
                    line: f.line.map(|l| l as u32),
                })
                .collect(),
            actions,
            open: Some(Open {
                app: APP.into(),
                args: vec!["--select".into(), f.id.clone()],
            }),
        });
    }
    report.notes.push(format!(
        "{} instruction file(s) read across {} project(s); {} host(s) present",
        inv.files.len(),
        inv.projects.len(),
        hosts_present
    ));
    let absent = inv.findings.len() - n;
    if absent > 0 {
        report.notes.push(format!(
            "{absent} finding(s) for hosts not on this machine are shown in the app only"
        ));
    }
    report.notes.extend(inv.notes.iter().cloned());
    report.errors.extend(inv.errors.iter().cloned());
    let _ = Kind::ALL;
    report
}
