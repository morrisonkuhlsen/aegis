//! dBASE III tables (`.dbf`): the attribute half of a shapefile.

use mkaegis_core::{Error, Result, Value};
use mkaegis_model::{Field, FieldType};

/// dBASE field names are at most 10 bytes.
const NAME_LEN: usize = 10;
const DESCRIPTOR: usize = 32;
/// Doubles are written as `N` 19.11, as is customary for shapefiles.
const DOUBLE_WIDTH: usize = 19;
const DOUBLE_DECIMALS: usize = 11;
/// Integers are written at least 18 wide, and up to 20 (`-9223372036854775808`) when a value
/// needs it; `N` columns without decimals up to this width read as integers.
const INT_WIDTH: usize = 18;
const INT_MAX_WIDTH: usize = 20;
/// Short integers: six characters hold -32768.
const SHORT_WIDTH: usize = 6;
/// The widest `C` column dBASE III holds.
const TEXT_MAX_WIDTH: usize = 254;
/// Date (`D`) columns: `YYYYMMDD`.
const DATE_WIDTH: usize = 8;

pub struct Table {
    pub fields: Vec<Field>,
    pub rows: Vec<Vec<Value>>,
}

/// A written table, and what writing it had to change.
pub struct Written {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
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
            b'N' | b'F' if len > INT_MAX_WIDTH => FieldType::Double,
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
                FieldType::Integer | FieldType::Short => {
                    t.parse::<i64>().map(Value::Int).or_else(|_| t.parse::<f64>().map(Value::Float)).unwrap_or(Value::Null)
                }
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
    // Narrow integer columns whose values all fit 16 bits are Short, as they were written (#55).
    for (i, (c, f)) in columns.iter().zip(fields.iter_mut()).enumerate() {
        let fits = |v: &Value| match v {
            Value::Null => true,
            Value::Int(x) => i16::try_from(*x).is_ok(),
            _ => false,
        };
        if f.kind == FieldType::Integer && c.len <= SHORT_WIDTH && rows.iter().all(|r| r.get(i).is_none_or(fits)) {
            f.kind = FieldType::Short;
        }
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

/// A double that fits `width` characters: fixed-point with as many decimals (up to
/// `decimals`) as fit, or in exponent notation with as many digits as fit when that is closer
/// (very large and very small values). The exponent is never cut off (#55).
fn fit_double(x: f64, width: usize, decimals: usize) -> String {
    let fixed = (0..=decimals).rev().map(|d| format!("{x:.d$}")).find(|s| s.len() <= width);
    let exponent = (0..=16).rev().map(|p| format!("{x:.p$e}")).find(|s| s.len() <= width);
    let error = |s: &String| s.parse::<f64>().map_or(f64::INFINITY, |y| (y - x).abs());
    let s = match (fixed, exponent) {
        (Some(f), Some(e)) => {
            if error(&e) < error(&f) {
                e
            } else {
                f
            }
        }
        (f, e) => f.or(e).unwrap_or_default(),
    };
    format!("{s:>width$}")
}

/// An integer column's text for `v`: whole numbers only, never through a double (#55).
fn integer_text(v: &Value) -> Option<String> {
    match v {
        Value::Int(i) => Some(i.to_string()),
        Value::Bool(b) => Some(u8::from(*b).to_string()),
        // 2^63 is the first double past i64.
        Value::Float(x) if x.is_finite() && x.round().abs() < 9.223_372_036_854_776e18 => Some(format!("{:.0}", x.round())),
        Value::Text(t) => t.trim().parse::<i64>().ok().map(|i| i.to_string()),
        _ => None,
    }
}

/// `YYYYMMDD` for a `YYYY-MM-DD` date, the form `D` columns are read into.
fn date_text(v: &Value) -> Option<String> {
    let Value::Text(t) = v else { return None };
    let b = t.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let ok = b.len() == 10 && b.get(4) == Some(&b'-') && b.get(7) == Some(&b'-') && digits(0..4) && digits(5..7) && digits(8..10);
    let (month, day) = (t.get(5..7)?.parse::<u8>().ok()?, t.get(8..10)?.parse::<u8>().ok()?);
    (ok && (1..=12).contains(&month) && (1..=31).contains(&day)).then(|| t.replace('-', ""))
}

/// A dBASE III table for `fields` and `rows`. Every value is written as it is or the column is
/// widened to hold it; text past 254 bytes, the format's limit, is cut and reported (#55).
pub fn write(fields: &[Field], rows: &[&[Value]]) -> Result<Written> {
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
    let column = |i: usize| rows.iter().filter_map(move |r| r.get(i)).filter(|v| !v.is_null());
    let widest = |i: usize, text: fn(&Value) -> Option<String>| column(i).filter_map(text).map(|s| s.len()).max().unwrap_or(0);
    let mut warnings = Vec::new();
    let columns: Vec<(u8, usize, u8)> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| match f.kind {
            FieldType::Integer => (b'N', widest(i, integer_text).clamp(INT_WIDTH, INT_MAX_WIDTH), 0),
            FieldType::Short => (b'N', widest(i, integer_text).clamp(SHORT_WIDTH, INT_MAX_WIDTH), 0),
            FieldType::Double => (b'N', DOUBLE_WIDTH, DOUBLE_DECIMALS as u8),
            FieldType::Boolean => (b'L', 1, 0),
            // Text that is all dates, as `D` columns are read, is written back as dates.
            FieldType::Text if column(i).next().is_some() && column(i).all(|v| date_text(v).is_some()) => (b'D', DATE_WIDTH, 0),
            FieldType::Text => {
                let width = column(i).map(|v| v.to_string().len()).max().unwrap_or(1);
                if width > TEXT_MAX_WIDTH {
                    let cut_rows = column(i).filter(|v| v.to_string().len() > TEXT_MAX_WIDTH).count();
                    warnings.push(format!("field `{}`: {cut_rows} values longer than {TEXT_MAX_WIDTH} bytes, the dBASE limit, were cut", f.name));
                }
                (b'C', width.clamp(1, TEXT_MAX_WIDTH), 0)
            }
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
            let blank = || " ".repeat(*len);
            let cell = match (kind, v) {
                (_, Value::Null) => blank(),
                (b'L', v) => if v.as_f64().is_some_and(|x| x != 0.0) { "T" } else { "F" }.to_string(),
                (b'N', v) if *dec == 0 => integer_text(v).map_or_else(blank, |s| format!("{s:>len$}", len = *len)),
                (b'N', v) => v.as_f64().map_or_else(blank, |x| fit_double(x, *len, usize::from(*dec))),
                (b'D', v) => date_text(v).unwrap_or_else(blank),
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
    Ok(Written { bytes: out, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_names_are_cut_and_made_unique() {
        let fields = [Field::new("POPULATION_2020", FieldType::Integer), Field::new("POPULATION_2021", FieldType::Integer)];
        let t = read(&write(&fields, &[&[Value::Int(1), Value::Int(2)]]).unwrap().bytes, None).unwrap();
        assert_eq!(t.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["POPULATION", "POPULATI_1"]);
        assert_eq!(t.rows[0], vec![Value::Int(1), Value::Int(2)]);
    }

    #[test]
    fn latin1_when_the_cpg_says_so() {
        let fields = [Field::new("N", FieldType::Text)];
        let mut bytes = write(&fields, &[&[Value::Text("a".into())]]).unwrap().bytes;
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

    /// #55: integers went through a double and were cut to 18 digits, the exponent of a large
    /// double was cut off, dates became text, long text was cut without a word, and Short
    /// fields read back as Integer.
    #[test]
    fn every_field_type_round_trips() {
        let fields = [
            Field::new("I", FieldType::Integer),
            Field::new("S", FieldType::Short),
            Field::new("D", FieldType::Double),
            Field::new("T", FieldType::Text),
            Field::new("B", FieldType::Boolean),
            Field::new("DAY", FieldType::Text),
        ];
        let rows: Vec<Vec<Value>> = vec![
            vec![
                Value::Int(i64::MAX),
                Value::Int(-32768),
                Value::Float(-1.234_567_891_234_568e23),
                Value::Text("São Paulo".into()),
                Value::Bool(true),
                Value::Text("2026-10-21".into()),
            ],
            vec![
                Value::Int(i64::MIN),
                Value::Int(32767),
                Value::Float(1.234_567_891_234_568e23),
                Value::Text(String::new()),
                Value::Bool(false),
                Value::Null,
            ],
            vec![
                Value::Int(1_234_567_890_123_456_789),
                Value::Int(0),
                Value::Float(1.5e-15),
                Value::Null,
                Value::Null,
                Value::Text("1999-01-31".into()),
            ],
            vec![Value::Int(-123_456_789_012_345_678), Value::Null, Value::Float(-0.25), Value::Text("x".into()), Value::Bool(true), Value::Null],
            vec![Value::Null, Value::Int(-1), Value::Float(1e300), Value::Text("y".into()), Value::Bool(false), Value::Null],
        ];
        let refs: Vec<&[Value]> = rows.iter().map(Vec::as_slice).collect();
        let written = write(&fields, &refs).unwrap();
        assert!(written.warnings.is_empty(), "{:?}", written.warnings);
        let t = read(&written.bytes, None).unwrap();
        assert_eq!(t.fields.iter().map(|f| f.kind).collect::<Vec<_>>(), fields.iter().map(|f| f.kind).collect::<Vec<_>>());
        for (r, (back, row)) in t.rows.iter().zip(&rows).enumerate() {
            for (c, (b, v)) in back.iter().zip(row).enumerate() {
                match (b, v) {
                    // 19 characters with a sign and a two-digit exponent hold 14 significant digits.
                    (Value::Float(b), Value::Float(v)) => assert!((b - v).abs() <= v.abs() * 1e-13, "row {r} col {c}: {b} vs {v}"),
                    // An empty text is NULL in dBASE.
                    (Value::Null, Value::Text(v)) if v.is_empty() => {}
                    _ => assert_eq!(b, v, "row {r} col {c}"),
                }
            }
        }
    }

    #[test]
    fn text_past_the_dbase_limit_is_cut_with_a_warning() {
        let fields = [Field::new("T", FieldType::Text)];
        let long = Value::Text("ã".repeat(200));
        let written = write(&fields, &[&[long], &[Value::Text("short".into())]]).unwrap();
        assert_eq!(written.warnings.len(), 1, "{:?}", written.warnings);
        assert!(written.warnings[0].contains("1 values longer than 254 bytes"), "{}", written.warnings[0]);
        let Value::Text(back) = &read(&written.bytes, None).unwrap().rows[0][0] else { panic!("not text") };
        assert_eq!(back, &"ã".repeat(127));
    }

    #[test]
    fn wide_or_out_of_range_integer_columns_stay_integer() {
        // A narrow column with a value past 16 bits, as other programs write, is not Short.
        let fields = [Field::new("I", FieldType::Short)];
        let bytes = write(&fields, &[&[Value::Int(1)], &[Value::Int(99_999)]]).unwrap().bytes;
        assert_eq!(read(&bytes, None).unwrap().fields[0].kind, FieldType::Integer);
        let fields = [Field::new("I", FieldType::Integer)];
        let bytes = write(&fields, &[&[Value::Int(1)]]).unwrap().bytes;
        assert_eq!(read(&bytes, None).unwrap().fields[0].kind, FieldType::Integer);
    }
}
