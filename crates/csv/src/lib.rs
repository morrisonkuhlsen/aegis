//! Delimited text tables (CSV, TSV, and the semicolon CSV of comma-decimal locales) in and out,
//! from RFC 4180: fields separated by a delimiter, records by line breaks, a field in double
//! quotes may hold delimiters, line breaks and doubled quotes.
//!
//! Reading guesses what files rarely state: the encoding (UTF-8, with or without a byte-order
//! mark, else Windows-1252), the delimiter (the one of `,` `;` tab `|` that splits the first
//! lines most consistently) and the decimal separator (a comma when the delimiter is `;` and the
//! numbers say so). Each column's type is inferred and can be overridden: whole numbers are
//! Integer, other numbers Double, `true`/`false` Boolean, the rest Text. **A number written with
//! a leading zero (`0012`, an IBGE or FIPS code) is Text**, so the zero survives.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::HashMap;

use mkaegis_core::{Error, MAX_ITEMS, Result, Value};
use mkaegis_model::{Field, FieldType, MAX_FIELD_NAME};

/// The most columns a table is read with.
pub const MAX_COLUMNS: usize = 4_096;
/// How many records the delimiter is guessed from.
const SNIFF_LINES: usize = 20;
const DELIMITERS: [char; 4] = [',', ';', '\t', '|'];

/// A table read from delimited text.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub fields: Vec<Field>,
    pub rows: Vec<Vec<Value>>,
    /// What was guessed, for the person to check.
    pub delimiter: char,
    pub decimal_comma: bool,
    pub encoding: &'static str,
    pub warnings: Vec<String>,
}

/// Reads a table from a file's bytes. `types` overrides inferred column types by column name
/// (case-insensitive), with the names `text`, `integer`, `double` and `boolean`.
pub fn read(bytes: &[u8], types: &HashMap<String, FieldType>) -> Result<Table> {
    let (text, encoding) = decode(bytes);
    let delimiter = sniff(&text);
    let mut records = parse(&text, delimiter)?;
    if records.is_empty() {
        return Err(Error::format("CSV", "the file is empty"));
    }
    let header = records.remove(0);
    if header.len() > MAX_COLUMNS {
        return Err(Error::format("CSV", format!("{} columns; up to {MAX_COLUMNS}", header.len())));
    }
    if records.len() > MAX_ITEMS {
        return Err(Error::format("CSV", format!("{} rows; up to {MAX_ITEMS}", records.len())));
    }
    let mut warnings = Vec::new();
    let names = field_names(&header, &mut warnings);
    let ragged = records.iter().filter(|r| r.len() != names.len()).count();
    if ragged > 0 {
        warnings.push(format!("{ragged} rows have more or fewer values than the header: padded with NULL or cut"));
    }
    let decimal_comma = delimiter != ',' && looks_decimal_comma(&records);
    let mut fields = Vec::with_capacity(names.len());
    for (c, name) in names.iter().enumerate() {
        let column = records.iter().map(|r| r.get(c).map_or("", String::as_str));
        let kind = match types.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
            Some((_, kind)) => *kind,
            None => infer(column, decimal_comma),
        };
        fields.push(Field::new(name, kind));
    }
    let mut unparsed = 0usize;
    let rows: Vec<Vec<Value>> = records
        .iter()
        .map(|r| {
            fields
                .iter()
                .enumerate()
                .map(|(c, f)| {
                    let raw = r.get(c).map_or("", String::as_str);
                    let v = value(raw, f.kind, decimal_comma);
                    if v.is_null() && !raw.trim().is_empty() {
                        unparsed += 1;
                    }
                    v
                })
                .collect()
        })
        .collect();
    if unparsed > 0 {
        warnings.push(format!("{unparsed} values didn't fit their column's type and were read as NULL"));
    }
    Ok(Table { fields, rows, delimiter, decimal_comma, encoding, warnings })
}

/// A table as CSV: comma-delimited, UTF-8, quoted where a value needs it, NULL as empty, a
/// header of field names. Numbers are written in full with a decimal point.
pub fn write(fields: &[Field], rows: &[&[Value]]) -> String {
    let mut out = String::new();
    let line = |cells: Vec<String>| cells.join(",") + "\r\n";
    out.push_str(&line(fields.iter().map(|f| quote(&f.name)).collect()));
    for row in rows {
        let cells = fields
            .iter()
            .enumerate()
            .map(|(i, _)| match row.get(i) {
                None | Some(Value::Null) => String::new(),
                Some(Value::Float(x)) => format!("{x}"),
                Some(v) => quote(&v.to_string()),
            })
            .collect();
        out.push_str(&line(cells));
    }
    out
}

/// Parses a type override's name.
pub fn parse_type(text: &str) -> Option<FieldType> {
    match text.to_ascii_lowercase().as_str() {
        "text" | "string" => Some(FieldType::Text),
        "integer" | "long" | "int" => Some(FieldType::Integer),
        "double" | "float" | "number" => Some(FieldType::Double),
        "boolean" | "bool" => Some(FieldType::Boolean),
        _ => None,
    }
}

/// UTF-8 (a byte-order mark dropped), else Windows-1252 byte by byte.
fn decode(bytes: &[u8]) -> (String, &'static str) {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), "UTF-8"),
        Err(_) => (bytes.iter().map(|b| cp1252(*b)).collect(), "Windows-1252"),
    }
}

/// A Windows-1252 byte as a character: Latin-1, with the printable characters of 0x80–0x9F.
fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—',
        '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    if (0x80..0xa0).contains(&b) { HIGH.get(usize::from(b - 0x80)).copied().unwrap_or('?') } else { char::from(b) }
}

/// The delimiter that splits the first records into the same number of fields (more than one),
/// the most often; a comma when none does.
fn sniff(text: &str) -> char {
    let mut best = (',', 0usize, 0usize);
    for d in DELIMITERS {
        let Ok(records) = parse_limited(text, d, SNIFF_LINES) else { continue };
        let Some(first) = records.first().map(Vec::len) else { continue };
        if first < 2 {
            continue;
        }
        let agree = records.iter().filter(|r| r.len() == first).count();
        if (agree, first) > (best.1, best.2) {
            best = (d, agree, first);
        }
    }
    best.0
}

fn parse(text: &str, delimiter: char) -> Result<Vec<Vec<String>>> {
    parse_limited(text, delimiter, usize::MAX)
}

/// Records of at most `limit` lines, by RFC 4180's rules; blank lines are skipped.
fn parse_limited(text: &str, delimiter: char, limit: usize) -> Result<Vec<Vec<String>>> {
    let mut records = Vec::new();
    let (mut record, mut field) = (Vec::new(), String::new());
    let (mut quoted, mut was_quoted) = (false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !was_quoted => (quoted, was_quoted) = (true, true),
            c if c == delimiter => {
                record.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                record.push(std::mem::take(&mut field));
                was_quoted = false;
                if !(record.len() == 1 && record.first().is_some_and(String::is_empty)) {
                    records.push(std::mem::take(&mut record));
                } else {
                    record.clear();
                }
                if records.len() >= limit {
                    return Ok(records);
                }
                if record.len() > MAX_COLUMNS {
                    return Err(Error::format("CSV", format!("more than {MAX_COLUMNS} columns")));
                }
            }
            _ => field.push(c),
        }
        if record.len() > MAX_COLUMNS {
            return Err(Error::format("CSV", format!("more than {MAX_COLUMNS} columns")));
        }
    }
    if quoted {
        return Err(Error::format("CSV", "a quoted value is never closed"));
    }
    if !field.is_empty() || !record.is_empty() || was_quoted {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

/// Valid, unique field names: letters, digits and `_`, at most [`MAX_FIELD_NAME`] bytes.
fn field_names(header: &[String], warnings: &mut Vec<String>) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(header.len());
    let mut renamed = 0;
    for (i, raw) in header.iter().enumerate() {
        let clean: String = raw.trim().chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
        let mut base: String = clean.chars().take(MAX_FIELD_NAME - 4).collect();
        if base.is_empty() {
            base = format!("FIELD{}", i + 1);
        }
        let mut name = base.clone();
        let mut n = 1;
        while names.iter().any(|m| m.eq_ignore_ascii_case(&name)) {
            name = format!("{base}_{n}");
            n += 1;
        }
        if name != raw.trim() {
            renamed += 1;
        }
        names.push(name);
    }
    if renamed > 0 {
        warnings.push(format!("{renamed} column names were changed to letters, digits and _, or made unique"));
    }
    names
}

/// A semicolon or tab file whose number-like values use a comma before the decimals.
fn looks_decimal_comma(records: &[Vec<String>]) -> bool {
    let cells = records.iter().take(200).flatten().map(|c| c.trim());
    let comma = cells.clone().filter(|c| is_number(c, true) && c.contains(',')).count();
    let point = cells.filter(|c| c.contains('.') && !c.contains(',') && c.parse::<f64>().is_ok()).count();
    comma > 0 && comma >= point
}

fn is_number(text: &str, decimal_comma: bool) -> bool {
    number(text, decimal_comma).is_some()
}

/// A number, read with the file's decimal separator (thousands separators aren't read).
fn number(text: &str, decimal_comma: bool) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(|c| c.is_alphabetic() && !matches!(c, 'e' | 'E')) {
        return None;
    }
    let t = if decimal_comma {
        if t.contains('.') {
            return None;
        }
        t.replace(',', ".")
    } else {
        t.to_string()
    };
    t.parse::<f64>().ok().filter(|x| x.is_finite())
}

/// A whole number without a leading zero or sign quirks, so that writing it back gives the same
/// text: `0012` and `+5` are codes, not numbers.
fn integer(text: &str) -> Option<i64> {
    let t = text.trim();
    let digits = t.strip_prefix('-').unwrap_or(t);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    t.parse().ok()
}

fn infer<'a>(column: impl Iterator<Item = &'a str> + Clone, decimal_comma: bool) -> FieldType {
    let values: Vec<&str> = column.map(str::trim).filter(|v| !v.is_empty()).collect();
    if values.is_empty() {
        return FieldType::Text;
    }
    if values.iter().all(|v| integer(v).is_some()) {
        return FieldType::Integer;
    }
    // A leading zero before another digit is a code, even among decimals.
    let code = |v: &&str| {
        let d = v.strip_prefix('-').unwrap_or(v);
        d.len() > 1 && d.starts_with('0') && d.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
    };
    if values.iter().all(|v| number(v, decimal_comma).is_some()) && !values.iter().any(code) {
        return FieldType::Double;
    }
    if values.iter().all(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "false")) {
        return FieldType::Boolean;
    }
    FieldType::Text
}

fn value(raw: &str, kind: FieldType, decimal_comma: bool) -> Value {
    let t = raw.trim();
    if t.is_empty() {
        return Value::Null;
    }
    match kind {
        FieldType::Integer | FieldType::Short => t.parse::<i64>().map_or(Value::Null, Value::Int),
        FieldType::Double => number(t, decimal_comma).map_or(Value::Null, Value::Float),
        FieldType::Boolean => match t.to_ascii_lowercase().as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::Null,
        },
        // Text keeps the value as written, spaces inside quotes included.
        FieldType::Text => Value::Text(raw.to_string()),
    }
}

/// A CSV cell, quoted when it holds a delimiter, a quote, a line break or edge spaces.
fn quote(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) || text.starts_with(' ') || text.ends_with(' ') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_types() -> HashMap<String, FieldType> {
        HashMap::new()
    }

    #[test]
    fn types_codes_and_quotes() {
        let csv = "CODE,NAME,POP,RATE,OK\r\n0012,\"Lagoa, Alta\",1500,0.25,true\r\n3550308,\"Say \"\"hi\"\"\",12000000,1.5,false\r\n";
        let t = read(csv.as_bytes(), &no_types()).unwrap();
        let kinds: Vec<FieldType> = t.fields.iter().map(|f| f.kind).collect();
        assert_eq!(kinds, [FieldType::Text, FieldType::Text, FieldType::Integer, FieldType::Double, FieldType::Boolean]);
        assert_eq!(t.rows[0][0], Value::Text("0012".into()));
        assert_eq!(t.rows[0][1], Value::Text("Lagoa, Alta".into()));
        assert_eq!(t.rows[1][1], Value::Text("Say \"hi\"".into()));
        assert_eq!((t.delimiter, t.decimal_comma, t.encoding), (',', false, "UTF-8"));
        // The code survives a round trip.
        let rows: Vec<&[Value]> = t.rows.iter().map(Vec::as_slice).collect();
        let back = read(write(&t.fields, &rows).as_bytes(), &no_types()).unwrap();
        assert_eq!(back.rows, t.rows);
        assert_eq!(back.fields, t.fields);
    }

    #[test]
    fn semicolons_decimal_commas_and_latin1() {
        let mut bytes = b"MUNIC\xcdPIO;TAXA;COD\nS\xe3o Paulo;12,5;035\nCampinas;3,25;010\n".to_vec();
        let t = read(&bytes, &no_types()).unwrap();
        assert_eq!((t.delimiter, t.decimal_comma, t.encoding), (';', true, "Windows-1252"));
        assert_eq!(t.fields[0].name, "MUNICÍPIO");
        assert_eq!(t.rows[0][0], Value::Text("São Paulo".into()));
        assert_eq!(t.rows[1][1], Value::Float(3.25));
        assert_eq!(t.rows[0][2], Value::Text("035".into()));
        // A byte-order mark and tabs.
        bytes = b"\xef\xbb\xbfA\tB\n1\t2\n".to_vec();
        let t = read(&bytes, &no_types()).unwrap();
        assert_eq!((t.delimiter, t.fields[0].name.as_str()), ('\t', "A"));
    }

    #[test]
    fn overrides_names_and_ragged_rows() {
        let types: HashMap<String, FieldType> = [("id".to_string(), FieldType::Text)].into();
        let t = read(b"ID,Taxa (%),Taxa (%)\n1,2\n3,4,5,6\n", &types).unwrap();
        assert_eq!(t.fields[0].kind, FieldType::Text);
        assert_eq!(t.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["ID", "Taxa____", "Taxa_____1"]);
        assert_eq!(t.rows[0], vec![Value::Text("1".into()), Value::Int(2), Value::Null]);
        assert_eq!(t.warnings.len(), 2, "{:?}", t.warnings);
        assert_eq!(parse_type("Double"), Some(FieldType::Double));
    }

    #[test]
    fn hostile_files_never_panic() {
        let good = "A,B,C\n1,\"x\ny\",3\n4,5,6\n".as_bytes().to_vec();
        for cut in 0..good.len() {
            let _ = read(&good[..cut], &no_types());
        }
        for i in 0..good.len() {
            for b in [b'"', b',', b'\n', 0xff, b'0'] {
                let mut bad = good.clone();
                bad[i] = b;
                let _ = read(&bad, &no_types());
            }
        }
        assert!(read(b"", &no_types()).is_err());
        assert!(read(b"A,B\n\"open,1\n", &no_types()).is_err());
        let wide = vec!["x"; MAX_COLUMNS + 1].join(",");
        assert!(read(wide.as_bytes(), &no_types()).is_err());
    }
}
