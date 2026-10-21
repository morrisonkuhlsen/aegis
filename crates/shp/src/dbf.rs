//! dBASE III tables (`.dbf`): the attribute half of a shapefile.

use mkaegis_core::{Error, Result, Value};
use mkaegis_model::{Field, FieldType};

/// dBASE field names are at most 10 bytes.
const NAME_LEN: usize = 10;
const DESCRIPTOR: usize = 32;
/// Doubles are written as `N` 19.11, as is customary for shapefiles.
const DOUBLE_WIDTH: usize = 19;
const DOUBLE_DECIMALS: usize = 11;
const INT_WIDTH: usize = 18;

pub struct Table {
    pub fields: Vec<Field>,
    pub rows: Vec<Vec<Value>>,
}

struct Column {
    kind: u8,
    len: usize,
}

fn le_u16(b: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?])))
}

fn le_u32(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_le_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?])).ok()
}

/// Text in the table's code page: UTF-8, else Latin-1 byte by byte.
fn decode(bytes: &[u8], utf8: bool) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) if utf8 => s.to_string(),
        _ => bytes.iter().map(|b| char::from(*b)).collect(),
    }
}

pub fn read(b: &[u8], cpg: Option<&str>) -> Result<Table> {
    let bad = |why: &str| Error::format("dBASE table", why.to_string());
    let utf8 = cpg.is_none_or(|c| {
        let c = c.trim().to_ascii_uppercase();
        c.is_empty() || c.contains("UTF") && c.contains('8') || c == "65001"
    });
    let records = le_u32(b, 4).ok_or_else(|| bad("truncated header"))?;
    let header_len = le_u16(b, 8).ok_or_else(|| bad("truncated header"))?;
    let record_len = le_u16(b, 10).ok_or_else(|| bad("truncated header"))?;
    if header_len < DESCRIPTOR + 1 || header_len > b.len() {
        return Err(bad("bad header length"));
    }
    let mut columns = Vec::new();
    let mut fields: Vec<Field> = Vec::new();
    let mut at = DESCRIPTOR;
    while at + DESCRIPTOR <= header_len && b.get(at) != Some(&0x0d) {
        let d = b.get(at..at + DESCRIPTOR).ok_or_else(|| bad("truncated field descriptor"))?;
        let raw = d.get(..11).unwrap_or_default();
        let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
        let mut name = decode(raw.get(..end).unwrap_or_default(), utf8).trim().to_string();
        if name.is_empty() || fields.iter().any(|f| f.name.eq_ignore_ascii_case(&name)) {
            name = format!("FIELD{}", fields.len() + 1);
        }
        let (kind, len, decimals) =
            (d.get(11).copied().unwrap_or(b'C'), usize::from(d.get(16).copied().unwrap_or(0)), d.get(17).copied().unwrap_or(0));
        let ftype = match kind {
            b'N' | b'F' if decimals > 0 => FieldType::Double,
            b'N' | b'F' if len > INT_WIDTH => FieldType::Double,
            b'N' | b'F' => FieldType::Integer,
            b'L' => FieldType::Boolean,
            _ => FieldType::Text,
        };
        fields.push(Field { name, kind: ftype });
        columns.push(Column { kind, len });
        at += DESCRIPTOR;
    }
    let widths: usize = columns.iter().map(|c| c.len).sum::<usize>() + 1;
    if record_len < widths {
        return Err(bad("record length shorter than its fields"));
    }
    // Trust the bytes present over the declared count.
    let available = b.len().saturating_sub(header_len) / record_len.max(1);
    let mut rows = Vec::with_capacity(records.min(available));
    for r in 0..records.min(available) {
        let start = header_len + r * record_len;
        let rec = b.get(start..start + record_len).ok_or_else(|| bad("truncated record"))?;
        if rec.first() == Some(&b'*') {
            // A deleted record still has its shape: keep the row, empty.
            rows.push(vec![Value::Null; columns.len()]);
            continue;
        }
        let mut pos = 1;
        let mut row = Vec::with_capacity(columns.len());
        for (c, f) in columns.iter().zip(&fields) {
            let cell = rec.get(pos..pos + c.len).unwrap_or_default();
            pos += c.len;
            let text = decode(cell, utf8);
            let t = text.trim();
            row.push(match f.kind {
                _ if t.is_empty() || t.chars().all(|ch| ch == '*') => Value::Null,
                FieldType::Integer => t.parse::<i64>().map(Value::Int).or_else(|_| t.parse::<f64>().map(Value::Float)).unwrap_or(Value::Null),
                FieldType::Double => t.parse::<f64>().ok().filter(|x| x.is_finite()).map_or(Value::Null, Value::Float),
                FieldType::Boolean => match t.chars().next().map(|ch| ch.to_ascii_uppercase()) {
                    Some('T' | 'Y') => Value::Bool(true),
                    Some('F' | 'N') => Value::Bool(false),
                    _ => Value::Null,
                },
                FieldType::Text if c.kind == b'D' && t.len() == 8 => {
                    Value::Text(format!("{}-{}-{}", t.get(..4).unwrap_or_default(), t.get(4..6).unwrap_or_default(), t.get(6..).unwrap_or_default()))
                }
                FieldType::Text => Value::Text(t.to_string()),
            });
        }
        rows.push(row);
    }
    Ok(Table { fields, rows })
}

/// A string cut to at most `max` bytes on a character boundary.
fn cut(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default()
}

/// A double that fits `width` characters, with as many decimals (up to `decimals`) as fit.
fn fit_double(x: f64, width: usize, decimals: usize) -> String {
    for d in (0..=decimals).rev() {
        let s = format!("{x:.d$}");
        if s.len() <= width {
            return format!("{s:>width$}");
        }
    }
    let s = format!("{x:e}");
    format!("{:>width$}", cut(&s, width))
}

pub fn write(fields: &[Field], rows: &[&[Value]]) -> Result<Vec<u8>> {
    // Unique names of at most 10 bytes.
    let mut names: Vec<String> = Vec::with_capacity(fields.len());
    for f in fields {
        let base = cut(&f.name, NAME_LEN).to_string();
        let mut name = base.clone();
        let mut n = 1;
        while names.iter().any(|m| m.eq_ignore_ascii_case(&name)) {
            let suffix = format!("_{n}");
            name = format!("{}{suffix}", cut(&base, NAME_LEN - suffix.len()));
            n += 1;
        }
        names.push(name);
    }
    let text_width =
        |i: usize| rows.iter().filter_map(|r| r.get(i)).map(|v| if v.is_null() { 0 } else { v.to_string().len() }).max().unwrap_or(1).clamp(1, 254);
    let columns: Vec<(u8, usize, u8)> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| match f.kind {
            FieldType::Integer => (b'N', INT_WIDTH, 0),
            FieldType::Double => (b'N', DOUBLE_WIDTH, DOUBLE_DECIMALS as u8),
            FieldType::Boolean => (b'L', 1, 0),
            FieldType::Text => (b'C', text_width(i), 0),
        })
        .collect();
    let header_len = DESCRIPTOR + DESCRIPTOR * fields.len() + 1;
    let record_len = 1 + columns.iter().map(|c| c.1).sum::<usize>();
    let (Ok(header_len16), Ok(record_len16), Ok(count)) = (u16::try_from(header_len), u16::try_from(record_len), u32::try_from(rows.len())) else {
        return Err(Error::param("fields", "too many fields or rows for a dBASE table"));
    };
    let mut out = Vec::with_capacity(header_len + record_len * rows.len() + 1);
    out.push(0x03);
    // Last update, YY MM DD: a fixed date keeps output reproducible.
    out.extend_from_slice(&[126, 1, 1]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&header_len16.to_le_bytes());
    out.extend_from_slice(&record_len16.to_le_bytes());
    out.extend_from_slice(&[0u8; 20]);
    for (name, (kind, len, dec)) in names.iter().zip(&columns) {
        let mut d = [0u8; DESCRIPTOR];
        for (slot, byte) in d.iter_mut().zip(name.bytes().take(NAME_LEN)) {
            *slot = byte;
        }
        d[11] = *kind;
        d[16] = u8::try_from(*len).unwrap_or(254);
        d[17] = *dec;
        out.extend_from_slice(&d);
    }
    out.push(0x0d);
    for row in rows {
        out.push(b' ');
        for (i, (kind, len, dec)) in columns.iter().enumerate() {
            let v = row.get(i).unwrap_or(&Value::Null);
            let cell = match (kind, v) {
                (_, Value::Null) => " ".repeat(*len),
                (b'L', v) => if v.as_f64().is_some_and(|x| x != 0.0) { "T" } else { "F" }.to_string(),
                (b'N', v) if *dec == 0 => match v.as_f64() {
                    Some(x) => format!("{:>len$}", cut(&format!("{}", x.round() as i64), *len), len = *len),
                    None => " ".repeat(*len),
                },
                (b'N', v) => v.as_f64().map_or_else(|| " ".repeat(*len), |x| fit_double(x, *len, usize::from(*dec))),
                (_, v) => {
                    let s = v.to_string();
                    let s = cut(&s, *len);
                    format!("{s}{}", " ".repeat(len - s.len()))
                }
            };
            out.extend_from_slice(cell.as_bytes());
        }
    }
    out.push(0x1a);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_names_are_cut_and_made_unique() {
        let fields = [Field::new("POPULATION_2020", FieldType::Integer), Field::new("POPULATION_2021", FieldType::Integer)];
        let t = read(&write(&fields, &[&[Value::Int(1), Value::Int(2)]]).unwrap(), None).unwrap();
        assert_eq!(t.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["POPULATION", "POPULATI_1"]);
        assert_eq!(t.rows[0], vec![Value::Int(1), Value::Int(2)]);
    }

    #[test]
    fn latin1_when_the_cpg_says_so() {
        let fields = [Field::new("N", FieldType::Text)];
        let mut bytes = write(&fields, &[&[Value::Text("a".into())]]).unwrap();
        let at = bytes.len() - 2;
        bytes[at] = 0xe3; // ã in Latin-1
        assert_eq!(read(&bytes, Some("ISO-8859-1")).unwrap().rows[0][0], Value::Text("ã".into()));
        // Invalid UTF-8 falls back to Latin-1 rather than failing.
        assert_eq!(read(&bytes, None).unwrap().rows[0][0], Value::Text("ã".into()));
    }

    #[test]
    fn doubles_fit_their_width() {
        assert_eq!(fit_double(1.5, 19, 11).trim(), "1.50000000000");
        assert_eq!(fit_double(123_456_789_012.5, 19, 11).trim(), "123456789012.500000");
        assert!(fit_double(1e300, 19, 11).len() <= 19);
    }
}
