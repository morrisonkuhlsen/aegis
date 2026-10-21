//! About ▸ Contributors and Models, compiled in from `contributors/`: commit stats from
//! `contributors.json` (generated) and the names people chose in `people.toml`.

use serde_json::Value as Json;

const STATS: &str = include_str!("../../../contributors/contributors.json");
const PEOPLE: &str = include_str!("../../../contributors/people.toml");

/// A person's chosen name from `people.toml`, if they added one.
fn chosen_name(login: &str) -> Option<String> {
    let header = format!("[people.{login}]");
    let mut inside = false;
    let (mut display, mut real) = (None, None);
    for line in PEOPLE.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
        if line.starts_with('[') {
            inside = line == header;
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| inside) else { continue };
        let value = value.split('#').next().unwrap_or("").trim().trim_matches('"').to_string();
        match key.trim() {
            "display_name" if !value.is_empty() => display = Some(value),
            "real_name" if !value.is_empty() => real = Some(value),
            _ => {}
        }
    }
    display.or(real)
}

/// (name shown, commits) for people, then for models.
pub fn credits() -> (Vec<(String, u64)>, Vec<(String, u64)>) {
    let stats: Json = serde_json::from_str(STATS).unwrap_or(Json::Null);
    let list = |key: &str, name: &dyn Fn(&Json) -> Option<String>| -> Vec<(String, u64)> {
        stats
            .get(key)
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| Some((name(e)?, e.get("commits").and_then(Json::as_u64).unwrap_or(0))))
            .collect()
    };
    let people = list("people", &|e| {
        let login = e.get("login").and_then(Json::as_str)?;
        Some(chosen_name(login).map_or_else(|| format!("@{login}"), |n| format!("{n} (@{login})")))
    });
    let models = list("models", &|e| e.get("name").and_then(Json::as_str).map(str::to_string));
    (people, models)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_compiled_in_files_parse() {
        let _ = super::credits();
        assert_eq!(super::chosen_name("nobody-here"), None);
    }
}
