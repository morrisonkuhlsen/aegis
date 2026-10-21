//! Weights as text: GAL (neighbour lists) and GWT (weighted pairs), the formats GeoDa
//! introduced and other spatial statistics software reads. Features are named by an id: the
//! values of an id field, or 1-based row numbers.
//!
//! GAL: a header line (`0 n layer id_field`, or just `n`), then for each feature a line
//! `id count` and a line with its neighbours' ids. GWT: the same header, then `id neighbour
//! weight` lines.

use std::collections::{BTreeMap, HashMap};

use mkaegis_core::{Error, Result};

/// The most features or lines a weights file is read with.
const MAX_ROWS: usize = 2_000_000;
const MAX_LINKS: usize = crate::weights::MAX_LINKS;

/// GAL text for `neighbours`, ids by row.
pub fn write_gal(neighbours: &[Vec<(u32, f64)>], ids: &[String], layer: &str, id_field: &str) -> String {
    let mut out = format!("0 {} {} {}\n", neighbours.len(), token(layer), token(id_field));
    for (i, row) in neighbours.iter().enumerate() {
        out.push_str(&format!("{} {}\n", id(ids, i), row.len()));
        let list: Vec<String> = row.iter().map(|(j, _)| id(ids, *j as usize)).collect();
        out.push_str(&list.join(" "));
        out.push('\n');
    }
    out
}

/// GWT text for `neighbours`, ids by row.
pub fn write_gwt(neighbours: &[Vec<(u32, f64)>], ids: &[String], layer: &str, id_field: &str) -> String {
    let mut out = format!("0 {} {} {}\n", neighbours.len(), token(layer), token(id_field));
    for (i, row) in neighbours.iter().enumerate() {
        for (j, w) in row {
            out.push_str(&format!("{} {} {}\n", id(ids, i), id(ids, *j as usize), w));
        }
    }
    out
}

/// Neighbour lists from GAL text, for features whose ids are `ids` (by row). Every weight is 1.
pub fn read_gal(text: &str, ids: &[String]) -> Result<Vec<Vec<(u32, f64)>>> {
    let index = id_index(ids)?;
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let n = header(lines.next(), ids.len())?;
    let mut out = vec![Vec::new(); ids.len()];
    let mut links = 0usize;
    for _ in 0..n {
        let Some(line) = lines.next() else { break };
        let mut head = line.split_whitespace();
        let (Some(who), Some(count)) = (head.next(), head.next()) else {
            return Err(bad(format!("`{line}`: expected an id and a neighbour count")));
        };
        let row = row_of(&index, who)?;
        let count: usize = count.parse().map_err(|_| bad(format!("`{count}` is not a neighbour count")))?;
        let list: Vec<&str> = if count == 0 { Vec::new() } else { lines.next().unwrap_or("").split_whitespace().collect() };
        if list.len() != count {
            return Err(bad(format!("`{who}` lists {} neighbours, not {count}", list.len())));
        }
        links = links.saturating_add(count);
        if links > MAX_LINKS {
            return Err(bad(format!("more than {MAX_LINKS} links")));
        }
        let mut row_list = Vec::with_capacity(count);
        for other in list {
            row_list.push((row_of(&index, other)?, 1.0));
        }
        row_list.sort_by_key(|w| w.0);
        row_list.dedup_by_key(|w| w.0);
        if let Some(slot) = out.get_mut(row as usize) {
            *slot = row_list;
        }
    }
    Ok(out)
}

/// Weighted neighbour lists from GWT text, for features whose ids are `ids` (by row).
pub fn read_gwt(text: &str, ids: &[String]) -> Result<Vec<Vec<(u32, f64)>>> {
    let index = id_index(ids)?;
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    header(lines.next(), ids.len())?;
    let mut rows: Vec<BTreeMap<u32, f64>> = vec![BTreeMap::new(); ids.len()];
    let mut links = 0usize;
    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let [a, b, w] = parts.as_slice() else {
            return Err(bad(format!("`{line}`: expected `id neighbour weight`")));
        };
        let w: f64 = w.parse().ok().filter(|w: &f64| w.is_finite()).ok_or_else(|| bad(format!("`{w}` is not a finite weight")))?;
        let (i, j) = (row_of(&index, a)?, row_of(&index, b)?);
        links = links.saturating_add(1);
        if links > MAX_LINKS {
            return Err(bad(format!("more than {MAX_LINKS} links")));
        }
        if let Some(row) = rows.get_mut(i as usize) {
            row.insert(j, w);
        }
    }
    Ok(rows.into_iter().map(|r| r.into_iter().collect()).collect())
}

/// The header's feature count, which must match the layer's.
fn header(line: Option<&str>, rows: usize) -> Result<usize> {
    let line = line.ok_or_else(|| bad("empty file".into()))?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    let n = match parts.as_slice() {
        ["0", n, ..] => n,
        [n, ..] => n,
        [] => return Err(bad("empty header".into())),
    };
    let n: usize = n.parse().map_err(|_| bad(format!("header `{line}`: no feature count")))?;
    if n > MAX_ROWS {
        return Err(bad(format!("{n} features; up to {MAX_ROWS}")));
    }
    if n != rows {
        return Err(bad(format!("the file is for {n} features and the layer has {rows}")));
    }
    Ok(n)
}

fn id_index(ids: &[String]) -> Result<HashMap<&str, u32>> {
    let mut index = HashMap::with_capacity(ids.len());
    for (row, id) in ids.iter().enumerate() {
        let row = u32::try_from(row).map_err(|_| bad("too many features".into()))?;
        if index.insert(id.as_str(), row).is_some() {
            return Err(Error::param("idField", format!("the id `{id}` is not unique")));
        }
    }
    Ok(index)
}

fn row_of(index: &HashMap<&str, u32>, id: &str) -> Result<u32> {
    index.get(id).copied().ok_or_else(|| bad(format!("no feature has the id `{id}`")))
}

fn id(ids: &[String], row: usize) -> String {
    ids.get(row).cloned().unwrap_or_else(|| (row + 1).to_string())
}

/// A header token: no spaces.
fn token(s: &str) -> String {
    let t: String = s.split_whitespace().collect::<Vec<_>>().join("_");
    if t.is_empty() { "-".into() } else { t }
}

fn bad(message: String) -> Error {
    Error::param("path", format!("weights file: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gal_and_gwt_round_trip() {
        let ids: Vec<String> = ["A1", "B2", "C3"].iter().map(|s| s.to_string()).collect();
        let w = vec![vec![(1, 0.5), (2, 0.5)], vec![(0, 1.0)], vec![]];
        let gal = write_gal(&w, &ids, "my layer", "CODE");
        assert!(gal.starts_with("0 3 my_layer CODE\nA1 2\nB2 C3\nB2 1\nA1\nC3 0\n"), "{gal}");
        let back = read_gal(&gal, &ids).unwrap();
        assert_eq!(back, vec![vec![(1, 1.0), (2, 1.0)], vec![(0, 1.0)], vec![]]);
        let gwt = write_gwt(&w, &ids, "l", "CODE");
        assert_eq!(read_gwt(&gwt, &ids).unwrap(), w);
        // A plain `n` header works too.
        assert_eq!(read_gal("3\nA1 1\nC3\nB2 0\nC3 1\nA1\n", &ids).unwrap()[0], vec![(2, 1.0)]);
    }

    #[test]
    fn hostile_files_are_refused() {
        let ids: Vec<String> = (1..=3).map(|i| i.to_string()).collect();
        for text in [
            "",
            "0 4 l id\n",
            "0 999999999999 l id\n",
            "0 3 l id\n1 2\n2\n",
            "0 3 l id\n1 1\n9\n",
            "0 3 l id\n1 x\n",
            "0 3 l id\n1 2 NaN\n",
            "0 3 l id\n1 2\n",
        ] {
            assert!(read_gal(text, &ids).is_err() || read_gwt(text, &ids).is_err(), "{text:?}");
        }
        assert!(read_gwt("0 3 l id\n1 2 inf\n", &ids).is_err());
        let dup: Vec<String> = vec!["a".into(), "a".into()];
        assert!(read_gal("2\n", &dup).is_err());
    }
}
