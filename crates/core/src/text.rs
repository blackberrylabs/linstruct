//! Prose helpers: front matter, code fences, `@` references, HTML comments, paragraphs,
//! secrets on a line. Pure functions on text; nothing here touches the disk.

use std::sync::OnceLock;

use bbl_hostrules::read::parse;
use bbl_hostrules::rules::Format;
use bbl_hostrules::secrets;
use regex::Regex;
use serde_json::Value;

/// The YAML front matter of a Markdown file, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatterText {
    /// Bytes before the opening `---` (0 when it starts the file).
    pub offset: usize,
    pub yaml: String,
    /// Line of the opening `---`, 1-based.
    pub line: usize,
    /// Byte where the body starts.
    pub body_start: usize,
}

/// Find a `---` block at the start of the text (leading blank lines tolerated, reported
/// through `offset`). Returns `Some(Err(..))` when the block is opened and never closed.
pub fn front_matter(text: &str) -> Option<Result<FrontMatterText, String>> {
    let trimmed_start = text.len() - text.trim_start().len();
    let rest = &text[trimmed_start..];
    if !(rest.starts_with("---\n") || rest.starts_with("---\r\n") || rest == "---") {
        return None;
    }
    let after_open = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
    let body = &rest[after_open..];
    // closing fence: a line that is exactly `---` (or `...`)
    let mut pos = 0;
    for line in body.split_inclusive('\n') {
        let l = line.trim_end_matches(['\n', '\r']);
        if l == "---" || l == "..." {
            let yaml = body[..pos].to_string();
            let line_no = text[..trimmed_start].matches('\n').count() + 1;
            return Some(Ok(FrontMatterText {
                offset: trimmed_start,
                yaml,
                line: line_no,
                body_start: trimmed_start + after_open + pos + line.len(),
            }));
        }
        pos += line.len();
    }
    Some(Err("front matter opened with `---` and never closed".into()))
}

/// Parse a front matter's YAML into a JSON object. Non-object YAML is an error too. A bare
/// glob value (`globs: *.ts`, as Cursor rules are commonly written and accepted) is quoted
/// first: strict YAML would read `*` as an alias.
pub fn parse_front_matter(yaml: &str) -> Result<serde_json::Map<String, Value>, String> {
    let lenient: String = yaml
        .lines()
        .map(|l| match l.split_once(':') {
            Some((k, v))
                if !k.trim().is_empty()
                    && k.trim()
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                    && v.trim_start().starts_with('*') =>
            {
                format!("{k}: \"{}\"", v.trim().replace('"', "'"))
            }
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    match parse(&lenient, Format::Yaml) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(Value::Null) => Ok(serde_json::Map::new()),
        Ok(_) => Err("front matter is not a key: value mapping".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// A front matter field as a list of strings: a YAML list, or a comma-separated string.
pub fn field_list(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        Value::String(s) => s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        Value::Bool(b) => vec![b.to_string()],
        Value::Number(n) => vec![n.to_string()],
        _ => Vec::new(),
    }
}

/// Is the field set, in the sense a host gives it: present and not empty / not `false`.
pub fn field_set(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(_) => true,
    }
}

/// Replace fenced code blocks and inline code spans by spaces, keeping every newline so
/// line numbers survive. Hosts skip references and comments inside code.
pub fn blank_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let t = body.trim_start();
        let fence = if t.starts_with("```") {
            Some("```")
        } else if t.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        match (in_fence, fence) {
            (Some(open), Some(f)) if f == open => {
                in_fence = None;
                push_blank(&mut out, line);
            }
            (Some(_), _) => push_blank(&mut out, line),
            (None, Some(f)) => {
                in_fence = Some(f);
                push_blank(&mut out, line);
            }
            (None, None) => {
                // inline spans
                let mut in_span = false;
                for c in line.chars() {
                    if c == '`' {
                        in_span = !in_span;
                        out.push(' ');
                    } else if in_span && c != '\n' {
                        out.push(' ');
                    } else {
                        out.push(c);
                    }
                }
            }
        }
    }
    out
}

fn push_blank(out: &mut String, line: &str) {
    for c in line.chars() {
        out.push(if c == '\n' { '\n' } else { ' ' });
    }
}

/// One reference found in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub target: String,
    pub line: usize,
    pub quoted: bool,
}

fn at_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?:^|[\s(\[])@(?:"([^"\n]+)"|([^\s"'<>()\[\]]+))"#).unwrap())
}

fn kiro_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"#\[\[(?:file|folder):([^\]:]+)(?::[^\]]*)?\]\]").unwrap())
}

/// `@path` references outside code, in the order found. Emails, npm scopes and bare
/// handles are left out: a target must look like a path (a `/` or a `.`), carry no second
/// `@`, and not sit right after a word character.
pub fn references(text: &str, syntax: &str) -> Vec<Reference> {
    let blanked = blank_code(text);
    let mut out = Vec::new();
    if syntax == "#[[file:" {
        for (i, line) in blanked.lines().enumerate() {
            for c in kiro_re().captures_iter(line) {
                out.push(Reference {
                    target: c[1].trim().to_string(),
                    line: i + 1,
                    quoted: false,
                });
            }
        }
        return out;
    }
    for (i, line) in blanked.lines().enumerate() {
        for c in at_re().captures_iter(line) {
            let quoted = c.get(1).is_some();
            let raw = c
                .get(1)
                .or_else(|| c.get(2))
                .map(|m| m.as_str())
                .unwrap_or("");
            let target = raw
                .trim_end_matches(['.', ',', ';', ':', '!', '?'])
                .to_string();
            // a file: its last component carries an extension; `@types/node`, `@alice`
            // and `me@example.com` are not references
            let last = target.rsplit('/').next().unwrap_or("");
            if target.contains('@')
                || target.ends_with('/')
                || !last.contains('.')
                || last.starts_with('.') && last.len() == 1
            {
                continue;
            }
            out.push(Reference {
                target,
                line: i + 1,
                quoted,
            });
        }
    }
    out
}

fn comment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<!--.*?-->").unwrap())
}

/// HTML comments outside code: (line, length in bytes) of each.
pub fn html_comments(text: &str) -> Vec<(usize, usize)> {
    let blanked = blank_code(text);
    comment_re()
        .find_iter(&blanked)
        .map(|m| (blanked[..m.start()].matches('\n').count() + 1, m.len()))
        .collect()
}

/// Paragraphs worth comparing: blank-line separated, whitespace collapsed, at least
/// `min_len` characters. Returns (normalised text, first line number).
pub fn paragraphs(text: &str, min_len: usize) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let flush = |current: &mut Vec<&str>, start: usize, out: &mut Vec<(String, usize)>| {
        if current.is_empty() {
            return;
        }
        let joined = current.join(" ");
        let norm: String = joined
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if norm.chars().count() >= min_len {
            out.push((norm, start));
        }
        current.clear();
    };
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            flush(&mut current, start, &mut out);
        } else {
            if current.is_empty() {
                start = i + 1;
            }
            current.push(line.trim());
        }
    }
    flush(&mut current, start, &mut out);
    out
}

fn kv_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"([A-Za-z_][A-Za-z0-9_.-]{1,60})\s*[=:]\s*["'`]?([^\s"'`,;]{8,})"#).unwrap()
    })
}

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"[A-Za-z0-9_./+-]{16,}"#).unwrap())
}

/// Secrets on one line, masked: (masked form, what gave it away).
pub fn secrets_on_line(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in kv_re().captures_iter(line) {
        let key = &c[1];
        let value = &c[2];
        let m = secrets::classify(key, value);
        if m.kind == secrets::ValueKind::Secret && seen.insert(value.to_string()) {
            out.push((m.shown, format!("value of `{key}`")));
        }
    }
    for m in token_re().find_iter(line) {
        let tok = m.as_str().trim_end_matches(['.', ',', ')']);
        if seen.contains(tok) {
            continue;
        }
        if secrets::value_looks_secret(tok) && !secrets::is_reference(tok) && !tok.contains("://") {
            seen.insert(tok.to_string());
            out.push((secrets::mask(tok), "credential-shaped value".into()));
        }
    }
    out
}

/// Byte offset → 1-based line.
pub fn line_at(text: &str, byte: usize) -> usize {
    let b = byte.min(text.len());
    text.as_bytes()[..b].iter().filter(|&&c| c == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_is_found_with_its_offset() {
        let fm = front_matter("---\npaths: [\"src/**\"]\n---\n# Rule\n")
            .unwrap()
            .unwrap();
        assert_eq!(fm.offset, 0);
        assert_eq!(fm.yaml, "paths: [\"src/**\"]\n");
        assert_eq!(
            &"---\npaths: [\"src/**\"]\n---\n# Rule\n"[fm.body_start..],
            "# Rule\n"
        );
        let fm = front_matter("\n\n---\na: 1\n---\nbody").unwrap().unwrap();
        assert_eq!(fm.offset, 2);
        assert!(front_matter("# No front matter\n").is_none());
        assert!(front_matter("---\na: 1\nbody").unwrap().is_err());
    }

    #[test]
    fn references_skip_code_emails_and_scopes() {
        let t = "See @docs/style.md and @\"my file.md\" but not me@example.com nor @types/node\n```\n@in/code.md\n```\n`@inline/span.md` @README.md.";
        let refs = references(t, "@");
        let targets: Vec<_> = refs.iter().map(|r| r.target.as_str()).collect();
        assert_eq!(targets, ["docs/style.md", "my file.md", "README.md"]);
        assert!(refs[1].quoted);
        assert_eq!(refs[2].line, 5);
    }

    #[test]
    fn kiro_references_are_found() {
        let refs = references(
            "Use #[[file:docs/api.md]] and #[[file:src/a.ts:10-20]]",
            "#[[file:",
        );
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[1].target, "src/a.ts");
    }

    #[test]
    fn html_comments_outside_code_only() {
        let t = "a\n<!-- note -->\n```\n<!-- in code -->\n```\n";
        assert_eq!(html_comments(t), vec![(2, 13)]);
    }

    #[test]
    fn paragraphs_are_normalised() {
        let p = paragraphs(
            "Line one\nline  two\n\nshort\n\nAnother paragraph that is long enough to count here",
            10,
        );
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], ("line one line two".into(), 1));
        assert_eq!(p[1].1, 6);
    }

    #[test]
    fn secrets_on_a_line_are_masked() {
        let s = secrets_on_line("export OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456");
        assert_eq!(s.len(), 1);
        assert!(!s[0].0.contains("abcdefghijklmnop"));
        assert!(secrets_on_line("Use ${OPENAI_API_KEY} from your environment").is_empty());
        assert!(secrets_on_line("See https://example.com/docs/getting-started").is_empty());
    }
}
