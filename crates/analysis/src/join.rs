//! Join Field: copies a table's fields onto a layer's features by a common key, and reports
//! what matched. Keys are compared as text, trimmed, so a code stays a code (`0012` is not `12`);
//! a whole number written as a Double key (`3550308.0`) compares as `3550308`. The join is
//! many-to-one: several features may share a key; when the table repeats a key, its first row is
//! used and the repeat is reported.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use mkaegis_core::{Error, Result, Value};
use mkaegis_model::{FeatureClass, Field, MAX_FIELD_NAME};
use serde::Serialize;

/// What a join matched, so the person can judge it before trusting the joined values.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JoinReport {
    /// Features whose key was found in the table.
    pub matched: usize,
    /// Features with no key, or a key the table doesn't have: their new fields are NULL.
    pub unmatched: usize,
    /// Table rows whose key no feature has.
    pub unused: usize,
    /// Keys the table repeats (its first row was used), and how many rows repeat them.
    pub duplicate_keys: usize,
    pub duplicate_rows: usize,
    /// A few keys of each kind, to look for the cause (spaces, leading zeros, types).
    pub unmatched_examples: Vec<String>,
    pub unused_examples: Vec<String>,
    /// The fields added, as named on the layer.
    pub fields: Vec<String>,
}

const EXAMPLES: usize = 5;

/// Joins `table` (its `fields` and `rows`) onto `target` by `key` (on the layer) and `table_key`
/// (in the table), adding the table's `copy` fields (all but the key when empty).
pub fn join_field(
    target: &mut FeatureClass,
    key: &str,
    fields: &[Field],
    rows: &[Vec<Value>],
    table_key: &str,
    copy: &[String],
) -> Result<JoinReport> {
    let k = target.field_index(key).ok_or_else(|| Error::NotFound(format!("field `{key}` on the layer")))?;
    let tk = fields
        .iter()
        .position(|f| f.name.eq_ignore_ascii_case(table_key))
        .ok_or_else(|| Error::NotFound(format!("field `{table_key}` in the table")))?;
    let picked: Vec<usize> = if copy.is_empty() {
        (0..fields.len()).filter(|i| *i != tk).collect()
    } else {
        copy.iter()
            .map(|name| {
                fields.iter().position(|f| f.name.eq_ignore_ascii_case(name)).ok_or_else(|| Error::NotFound(format!("field `{name}` in the table")))
            })
            .collect::<Result<_>>()?
    };
    if picked.is_empty() {
        return Err(Error::param("fields", "the table has no field to join besides its key"));
    }
    // The table's rows by key; the first row of a repeated key wins.
    let mut index: HashMap<String, usize> = HashMap::with_capacity(rows.len());
    let mut repeats: HashMap<String, usize> = HashMap::new();
    for (r, row) in rows.iter().enumerate() {
        let Some(key) = row.get(tk).and_then(key_text) else { continue };
        match index.entry(key) {
            Entry::Occupied(e) => *repeats.entry(e.key().clone()).or_insert(0) += 1,
            Entry::Vacant(e) => {
                e.insert(r);
            }
        }
    }
    let mut used = vec![false; rows.len()];
    let mut matches: Vec<Option<usize>> = Vec::with_capacity(target.features.len());
    let mut unmatched_examples = Vec::new();
    for f in &target.features {
        let key = f.values.get(k).and_then(key_text);
        let row = key.as_ref().and_then(|key| index.get(key)).copied();
        if let Some(r) = row
            && let Some(u) = used.get_mut(r)
        {
            *u = true;
        } else if unmatched_examples.len() < EXAMPLES {
            unmatched_examples.push(key.unwrap_or_else(|| "NULL".into()));
        }
        matches.push(row);
    }
    // Add the fields, renamed where the layer already has the name.
    let mut added = Vec::with_capacity(picked.len());
    for &c in &picked {
        let Some(source) = fields.get(c) else { continue };
        let name = unique_name(target, &source.name);
        let i = target.add_field(Field::new(&name, source.kind))?;
        for (f, row) in target.features.iter_mut().zip(&matches) {
            let v = row.and_then(|r| rows.get(r)).and_then(|r| r.get(c)).cloned().unwrap_or(Value::Null);
            if let Some(slot) = f.values.get_mut(i) {
                *slot = v;
            }
        }
        added.push(name);
    }
    let matched = matches.iter().filter(|m| m.is_some()).count();
    let unused_rows: Vec<usize> = index.values().copied().filter(|r| !used.get(*r).copied().unwrap_or(false)).collect();
    let mut unused_examples: Vec<String> = unused_rows.iter().filter_map(|r| rows.get(*r)?.get(tk).and_then(key_text)).collect();
    unused_examples.sort();
    unused_examples.truncate(EXAMPLES);
    Ok(JoinReport {
        matched,
        unmatched: target.features.len() - matched,
        unused: unused_rows.len(),
        duplicate_keys: repeats.len(),
        duplicate_rows: repeats.values().sum(),
        unmatched_examples,
        unused_examples,
        fields: added,
    })
}

/// A key as compared: text trimmed; a whole Double without its `.0`; none for NULL or empty.
fn key_text(v: &Value) -> Option<String> {
    let text = match v {
        Value::Null => return None,
        Value::Float(x) if x.fract() == 0.0 && x.abs() < 9.0e15 => format!("{}", *x as i64),
        v => v.to_string(),
    };
    let t = text.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn unique_name(fc: &FeatureClass, base: &str) -> String {
    let base: String = base.chars().take(MAX_FIELD_NAME - 4).collect();
    if fc.field_index(&base).is_none() {
        return base;
    }
    (1..).map(|n| format!("{base}_{n}")).find(|n| fc.field_index(n).is_none()).unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkaegis_crs::Crs;
    use mkaegis_geom::GeometryType;
    use mkaegis_model::FieldType;

    /// A constructed case: features keyed 0012, 0013, 0013 (shared), 99 (not in the table) and
    /// NULL; the table has 0012, 0013, 0013 again (a duplicate), 12 (not 0012) and 0099.
    #[test]
    fn the_join_report_counts_exactly() {
        let mut fc =
            FeatureClass::new("m", GeometryType::Point, Crs::WGS84, vec![Field::new("COD", FieldType::Text), Field::new("POP", FieldType::Integer)]);
        for code in [Value::Text("0012".into()), Value::Text("0013".into()), Value::Text(" 0013 ".into()), Value::Text("99".into()), Value::Null] {
            fc.push(None, vec![code, Value::Int(1)]).unwrap();
        }
        let fields = vec![Field::new("CODE", FieldType::Text), Field::new("POP", FieldType::Double), Field::new("NAME", FieldType::Text)];
        let row = |k: &str, pop: f64, name: &str| vec![Value::Text(k.into()), Value::Float(pop), Value::Text(name.into())];
        let rows = vec![row("0012", 1.5, "a"), row("0013", 2.5, "b"), row("0013", 9.9, "dup"), row("12", 3.5, "not a"), row("0099", 4.5, "z")];
        let r = join_field(&mut fc, "COD", &fields, &rows, "code", &[]).unwrap();
        assert_eq!((r.matched, r.unmatched, r.unused, r.duplicate_keys, r.duplicate_rows), (3, 2, 2, 1, 1));
        assert_eq!(r.unmatched_examples, vec!["99", "NULL"]);
        assert_eq!(r.unused_examples, vec!["0099", "12"]);
        // POP exists on the layer: the joined one is POP_1.
        assert_eq!(r.fields, vec!["POP_1", "NAME"]);
        let name = fc.field_index("NAME").unwrap();
        let names: Vec<Value> = fc.features.iter().map(|f| f.values[name].clone()).collect();
        assert_eq!(names, vec![Value::Text("a".into()), Value::Text("b".into()), Value::Text("b".into()), Value::Null, Value::Null]);
    }

    #[test]
    fn numeric_keys_and_errors() {
        let mut fc = FeatureClass::new("m", GeometryType::Point, Crs::WGS84, vec![Field::new("ID", FieldType::Integer)]);
        fc.push(None, vec![Value::Int(3550308)]).unwrap();
        let fields = vec![Field::new("ID", FieldType::Double), Field::new("V", FieldType::Double)];
        let rows = vec![vec![Value::Float(3550308.0), Value::Float(7.0)]];
        let r = join_field(&mut fc, "ID", &fields, &rows, "ID", &["V".into()]).unwrap();
        assert_eq!(r.matched, 1);
        assert!(join_field(&mut fc, "NOPE", &fields, &rows, "ID", &[]).is_err());
        assert!(join_field(&mut fc, "ID", &fields, &rows, "NOPE", &[]).is_err());
        assert!(join_field(&mut fc, "ID", &fields, &rows, "ID", &["MISSING".into()]).is_err());
        assert!(join_field(&mut fc, "ID", &fields[..1], &rows, "ID", &[]).is_err());
    }
}
