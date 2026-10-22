//! The interface language (#37). Every string the UI shows goes through [`tr`] (or [`trf`], with
//! values), keyed by its English text: a catalogue (`i18n/pt-BR.txt`, one `English = Português`
//! per line) gives the translation, and English is the fallback. Where one English word means two
//! things a key carries its context after a bar (`Project|tool`, the verb, not the noun of the
//! Project menu), which English drops. Only what is shown changes:
//! command ids and parameters, data, files and the command line keep their canonical forms.
//!
//! The language is per thread (the UI runs on one), so tests rendering in different languages
//! can run side by side; [`crate::MapApp`] sets it at the start of every frame.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::LazyLock;

/// The Portuguese (Brazil) catalogue.
pub const PT_BR: &str = include_str!("../i18n/pt-BR.txt");

static PT_BR_CATALOGUE: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| catalogue(PT_BR));

thread_local! {
    static CURRENT: Cell<Language> = const { Cell::new(Language::English) };
}

/// A language the interface can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    PortugueseBrazil,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::PortugueseBrazil];

    /// The language shown now.
    pub fn current() -> Language {
        CURRENT.with(Cell::get)
    }

    /// Shows the interface in this language from now on (on this thread).
    pub fn set(self) {
        CURRENT.with(|c| c.set(self));
    }

    /// Its BCP 47 tag, as settings keep it.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::PortugueseBrazil => "pt-BR",
        }
    }

    /// The language a tag names (`pt-BR`, `pt_BR`, `pt`, `en-US`…), if the interface has it.
    /// Every Portuguese is shown in Brazilian Portuguese, the one there is.
    pub fn from_code(code: &str) -> Option<Language> {
        let primary = code.trim().split(['-', '_', '.']).next().unwrap_or_default().to_ascii_lowercase();
        match primary.as_str() {
            "en" => Some(Language::English),
            "pt" => Some(Language::PortugueseBrazil),
            _ => None,
        }
    }

    /// Its name in itself, as a language menu lists it.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::PortugueseBrazil => "Português (Brasil)",
        }
    }

    /// The separators numbers are shown with: (thousands, decimal).
    fn separators(self) -> (char, char) {
        match self {
            Language::English => (',', '.'),
            Language::PortugueseBrazil => ('.', ','),
        }
    }
}

/// The text in the current language: its translation, or the English it is keyed by (without a
/// `|context`).
pub fn tr(english: &str) -> &str {
    let plain = english.split_once('|').map_or(english, |(text, _)| text);
    match Language::current() {
        Language::English => plain,
        Language::PortugueseBrazil => PT_BR_CATALOGUE.get(english).copied().unwrap_or(plain),
    }
}

/// A command's label in the current language. Project the tool is not Project the menu.
pub fn command_label<'a>(id: &str, label: &'a str) -> &'a str {
    match id {
        "management.project" => tr("Project|tool"),
        _ => tr(label),
    }
}

/// [`tr`], then each `{name}` replaced by its value.
pub fn trf(english: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = tr(english).to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

/// A geometry type's name, as layer properties and Identify show it.
pub fn geometry_name(t: mkaegis_geom::GeometryType) -> &'static str {
    use mkaegis_geom::GeometryType as G;
    match t {
        G::Point => tr("Point"),
        G::Multipoint => tr("Multipoint"),
        G::Polyline => tr("Polyline"),
        G::Polygon => tr("Polygon"),
    }
}

/// A number with `decimals` places, the decimal mark the current language's (`1.5` or `1,5`).
/// Not grouped: for coordinates, statistics and table cells.
pub fn decimal(x: f64, decimals: usize) -> String {
    if !x.is_finite() {
        return "—".into();
    }
    let text = format!("{x:.decimals$}");
    let (_, mark) = Language::current().separators();
    if mark == '.' { text } else { text.replace('.', &mark.to_string()) }
}

/// A number as Rust writes it (`1.5`, `2.40e-5`), with the current language's decimal mark.
pub fn localize(number: String) -> String {
    let (_, mark) = Language::current().separators();
    if mark == '.' { number } else { number.replace('.', &mark.to_string()) }
}

/// A value as tables and Identify show it: numbers with the current language's decimal mark.
/// Editing shows the canonical form, which is what the commands read.
pub fn value_text(v: &mkaegis_core::Value) -> String {
    match v {
        mkaegis_core::Value::Float(_) => localize(v.to_string()),
        _ => v.to_string(),
    }
}

/// A compass letter in the current language: Portuguese writes East and West as L and O.
pub fn compass(c: char) -> char {
    match (Language::current(), c) {
        (Language::PortugueseBrazil, 'E') => 'L',
        (Language::PortugueseBrazil, 'W') => 'O',
        _ => c,
    }
}

/// A ribbon path (`Map ▸ Layer`), each part translated.
pub fn ribbon_path(path: &str) -> String {
    path.split(" ▸ ").map(tr).collect::<Vec<_>>().join(" ▸ ")
}

/// A whole number grouped by thousands, the current language's way (`2,983,532` or `2.983.532`).
pub fn grouped(x: f64) -> String {
    if !x.is_finite() {
        return "—".into();
    }
    let (sep, _) = Language::current().separators();
    let digits = format!("{:.0}", x.abs());
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    if x < 0.0 && digits.chars().any(|c| c != '0') { format!("-{out}") } else { out }
}

/// A catalogue's entries: `English = Translation` lines; blank lines and `#` comments are
/// skipped.
pub fn catalogue(text: &'static str) -> HashMap<&'static str, &'static str> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.trim_end(), v.trim_start()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_the_key_and_the_fallback() {
        Language::English.set();
        assert_eq!(tr("Add Data"), "Add Data");
        assert_eq!(tr("Project|tool"), "Project");
        Language::PortugueseBrazil.set();
        assert_eq!(tr("Add Data"), "Adicionar dados");
        assert_eq!((tr("Project"), tr("Project|tool")), ("Projeto", "Projetar"));
        assert_eq!(tr("not in any catalogue"), "not in any catalogue");
        assert_eq!(trf("{count} features", &[("count", &3)]), "3 feições");
        Language::English.set();
    }

    #[test]
    fn numbers_follow_the_language() {
        Language::English.set();
        assert_eq!((decimal(1234.5, 2), grouped(2_983_532.0)), ("1234.50".to_string(), "2,983,532".to_string()));
        Language::PortugueseBrazil.set();
        assert_eq!((decimal(1234.5, 2), grouped(2_983_532.0)), ("1234,50".to_string(), "2.983.532".to_string()));
        assert_eq!((grouped(-1234.0), grouped(-0.2), decimal(f64::NAN, 2)), ("-1.234".to_string(), "0".to_string(), "—".to_string()));
        Language::English.set();
    }

    #[test]
    fn codes_and_the_catalogue_parse() {
        assert_eq!(Language::from_code("pt-BR"), Some(Language::PortugueseBrazil));
        assert_eq!(Language::from_code("pt_PT.UTF-8"), Some(Language::PortugueseBrazil));
        assert_eq!(Language::from_code("en-US"), Some(Language::English));
        assert_eq!(Language::from_code("fr"), None);
        assert!(Language::ALL.iter().all(|l| Language::from_code(l.code()) == Some(*l)));
        let c = catalogue("# a comment\n\nOpen = Abrir\nA = B = C\n");
        assert_eq!((c.get("Open"), c.get("A"), c.len()), (Some(&"Abrir"), Some(&"B = C"), 2));
    }
}
