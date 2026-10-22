//! `cargo xtask i18n` (#37): every string the UI translates has its Portuguese, the catalogue
//! holds nothing the UI no longer asks for, and no visible string skips the lookup.
//!
//! The keys are the literals passed to `tr(…)` and `trf(…)` in `crates/ui-egui/src` (outside
//! tests), every command's label and the parts of its ribbon path (`Map ▸ Layer`). A visible
//! string is a literal passed straight to `label`, `button`, `heading`, `RichText::new`,
//! `on_hover_text`, `hint_text`, `selected_text`, `link`, `Window::new` or `Button::new`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const UI_SOURCES: &str = "crates/ui-egui/src";
const CATALOGUES: [&str; 1] = ["crates/ui-egui/i18n/pt-BR.txt"];
/// Calls whose first string argument is shown as it is.
const SINKS: [&str; 11] = [
    ".label(",
    ".button(",
    ".heading(",
    "RichText::new(",
    ".on_hover_text(",
    ".hint_text(",
    ".selected_text(",
    ".link(",
    "Window::new(",
    "Button::new(",
    ".small_button(",
];

pub fn check(root: &Path) -> Result<(), String> {
    let mut keys = BTreeSet::new();
    let mut bare = Vec::new();
    let dir = root.join(UI_SOURCES);
    let mut files: Vec<_> = std::fs::read_dir(&dir).map_err(|e| format!("{UI_SOURCES}: {e}"))?.flatten().map(|e| e.path()).collect();
    files.sort();
    for path in files.iter().filter(|p| p.extension().is_some_and(|e| e == "rs")) {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let body = text.split("#[cfg(test)]\nmod tests").next().unwrap_or_default();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        keys.extend(called_with(body, "tr("));
        keys.extend(called_with(body, "trf("));
        for (n, line) in body.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for sink in SINKS {
                for (at, _) in line.match_indices(sink) {
                    let rest = line.get(at + sink.len()..).unwrap_or_default().trim_start();
                    if let Some(lit) = literal(rest)
                        && lit.chars().any(char::is_alphabetic)
                    {
                        bare.push(format!("{name}:{}: \"{lit}\" is shown without tr()", n + 1));
                    }
                }
            }
        }
    }
    for c in mkaegis_engine::Session::commands() {
        keys.insert(c.label.to_string());
        keys.extend(c.ribbon.split(" ▸ ").map(str::to_string));
    }

    let mut problems = bare;
    for catalogue in CATALOGUES {
        let text = std::fs::read_to_string(root.join(catalogue)).map_err(|e| format!("{catalogue}: {e}"))?;
        let mut entries: BTreeMap<&str, &str> = BTreeMap::new();
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            match line.split_once(" = ") {
                Some((k, v)) if !v.trim().is_empty() => {
                    if entries.insert(k.trim_end(), v.trim_start()).is_some() {
                        problems.push(format!("{catalogue}:{}: `{}` twice", n + 1, k.trim_end()));
                    }
                }
                _ => problems.push(format!("{catalogue}:{}: not `English = translation`", n + 1)),
            }
        }
        for k in &keys {
            if k.contains(" = ") {
                problems.push(format!("`{k}` can't be a key: it holds ` = `"));
            } else if !entries.contains_key(k.as_str()) {
                problems.push(format!("{catalogue}: missing `{k}`"));
            }
        }
        for k in entries.keys().filter(|k| !keys.contains(**k)) {
            problems.push(format!("{catalogue}: `{k}` is not used any more"));
        }
        // A value placeholder the translation drops would show nothing where a number belongs.
        for (k, v) in &entries {
            for p in placeholders(k) {
                if !v.contains(&p) {
                    problems.push(format!("{catalogue}: the translation of `{k}` lacks {p}"));
                }
            }
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(format!("{} problems:\n{}", problems.len(), problems.join("\n"))) }
}

/// The string literals passed first to `call` (`tr(`), which rustfmt may put on the next line.
fn called_with(body: &str, call: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, _) in body.match_indices(call) {
        // `tr(` inside another name (`attr(`, `str(`) is not the lookup.
        let before = body.get(..at).and_then(|b| b.chars().last());
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        if let Some(lit) = literal(body.get(at + call.len()..).unwrap_or_default().trim_start()) {
            out.push(lit);
        }
    }
    out
}

/// The string literal `text` starts with, unescaped; `None` if it starts with something else.
fn literal(text: &str) -> Option<String> {
    let mut chars = text.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// `{name}` placeholders in a key.
fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        let name = &rest[open + 1..open + close];
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            out.push(format!("{{{name}}}"));
        }
        rest = &rest[open + close + 1..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_and_placeholders() {
        assert_eq!(literal(r#""a \"b\" c", x"#).as_deref(), Some(r#"a "b" c"#));
        assert_eq!(literal("x"), None);
        assert_eq!(called_with("tr(\"A\") + attr(\"B\") + crate::trf(\n    \"C {n}\",", "tr("), ["A"]);
        assert_eq!(called_with("crate::trf(\n    \"C {n}\",", "trf("), ["C {n}"]);
        assert_eq!(placeholders("{count} of {total} {}"), ["{count}", "{total}"]);
    }
}
