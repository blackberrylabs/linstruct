//! The promises every Blackberrylabs tool keeps, checked on this crate.

use std::path::Path;

#[test]
fn the_core_never_writes_and_never_reaches_the_network() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&src).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap_or("");
        for forbidden in [
            "fs::write",
            "File::create",
            "fs::remove",
            "fs::rename",
            "OpenOptions",
            "std::net",
            "TcpStream",
            "reqwest",
            "ureq",
            "Command::new",
        ] {
            assert!(
                !code.contains(forbidden),
                "{}: mentions {forbidden}",
                entry.path().display()
            );
        }
    }
}

#[test]
fn the_report_conforms() {
    let tmp = tempfile::tempdir().unwrap();
    let env = bbl_hostrules::env::Env::fixture(tmp.path(), bbl_hostrules::env::Os::Linux)
        .with_roots(vec![tmp.path().to_path_buf()]);
    let inv = linstruct_core::scan(&env);
    let report = linstruct_core::report::build(&inv, 1);
    let json = report.to_json();
    let back = bbl_report::conformance::check(&json).expect("the report conforms to bbl-report/1");
    assert_eq!(back.module.id, "linstruct");
    assert_eq!(back.findings.len(), inv.findings.len());
    for f in &back.findings {
        assert_eq!(f.open.as_ref().unwrap().app, "linstruct-app");
    }
}

#[test]
fn a_scan_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();
    std::fs::write(
        tmp.path().join("CLAUDE.md"),
        "# x\n\nOPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz0123456789\n",
    )
    .unwrap();
    let before: Vec<_> = walk(tmp.path());
    let env = bbl_hostrules::env::Env::fixture(tmp.path(), bbl_hostrules::env::Os::Linux)
        .with_roots(vec![tmp.path().to_path_buf()]);
    let inv = linstruct_core::scan(&env);
    // the secret never appears in clear in the inventory
    let json = serde_json::to_string(&inv).unwrap();
    assert!(!json.contains("abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(inv
        .findings
        .iter()
        .any(|f| f.kind == linstruct_core::Kind::Secret));
    assert_eq!(walk(tmp.path()), before);
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
