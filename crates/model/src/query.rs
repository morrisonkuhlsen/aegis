//! SQL `WHERE` clauses over a feature class's fields, as Select By Attributes and Definition
//! Queries write them: `POP > 1000 AND NAME LIKE 'São%'`, `"TYPE" IN ('a', 'b')`,
//! `AREA BETWEEN 10 AND 20`, `NAME IS NOT NULL`, arithmetic, `UPPER()`/`LOWER()`.
//! A comparison with Null is false, as in SQL.

use aegis_core::{Error, Result, Value};

use crate::FeatureClass;

/// How deep parentheses and `NOT`s may nest.
const MAX_DEPTH: usize = 64;
/// The longest clause we parse.
const MAX_LEN: usize = 64 * 1024;

/// A parsed clause, bound to a feature class's field indexes.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    expr: Expr,
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Field(usize),
    Lit(Value),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cmp(Box<Expr>, Op, Box<Expr>),
    Arith(Box<Expr>, char, Box<Expr>),
    IsNull(Box<Expr>, bool),
    Like(Box<Expr>, Box<Expr>, bool),
    In(Box<Expr>, Vec<Expr>, bool),
    Between(Box<Expr>, Box<Expr>, Box<Expr>, bool),
    Upper(Box<Expr>),
    Lower(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Quoted(String),
    Sym(&'static str),
}

impl Query {
    /// Parses `text` against `fc`'s fields.
    pub fn parse(text: &str, fc: &FeatureClass) -> Result<Query> {
        if text.len() > MAX_LEN {
            return Err(Error::Query("the clause is too long".into()));
        }
        let toks = lex(text)?;
        if toks.is_empty() {
            return Err(Error::Query("the clause is empty".into()));
        }
        let mut p = Parser { toks, i: 0, fc, depth: 0 };
        let expr = p.or()?;
        if let Some(t) = p.toks.get(p.i) {
            return Err(Error::Query(format!("unexpected {}", show(t))));
        }
        Ok(Query { expr })
    }

    /// Whether row `row` matches.
    pub fn matches(&self, fc: &FeatureClass, row: usize) -> bool {
        truthy(&eval(&self.expr, fc, row))
    }

    /// The clause's value for row `row`, as Calculate Field uses an expression (`POP / 1000`).
    pub fn evaluate(&self, fc: &FeatureClass, row: usize) -> Value {
        eval(&self.expr, fc, row)
    }

    /// Every matching row.
    pub fn select(&self, fc: &FeatureClass) -> Vec<usize> {
        (0..fc.features.len()).filter(|r| self.matches(fc, *r)).collect()
    }
}

fn show(t: &Tok) -> String {
    match t {
        Tok::Num(n) => format!("number {n}"),
        Tok::Str(s) => format!("'{s}'"),
        Tok::Ident(s) | Tok::Quoted(s) => format!("`{s}`"),
        Tok::Sym(s) => format!("`{s}`"),
    }
}

fn lex(text: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c.is_whitespace() {
            i += 1;
        } else if c == '\'' || c == '"' {
            // 'text' with '' for a quote; "field name".
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err(Error::Query("unterminated quote".into())),
                    Some(&q) if q == c => {
                        if chars.get(i + 1) == Some(&c) {
                            s.push(c);
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            out.push(if c == '\'' { Tok::Str(s) } else { Tok::Quoted(s) });
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while chars.get(i).is_some_and(|d| d.is_ascii_digit() || *d == '.') {
                i += 1;
            }
            if chars.get(i).is_some_and(|e| *e == 'e' || *e == 'E') {
                i += 1;
                if chars.get(i).is_some_and(|s| *s == '+' || *s == '-') {
                    i += 1;
                }
                while chars.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
            }
            let s: String = chars.get(start..i).unwrap_or_default().iter().collect();
            out.push(Tok::Num(s.parse().map_err(|_| Error::Query(format!("bad number `{s}`")))?));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while chars.get(i).is_some_and(|d| d.is_alphanumeric() || *d == '_') {
                i += 1;
            }
            out.push(Tok::Ident(chars.get(start..i).unwrap_or_default().iter().collect()));
        } else {
            let two: String = chars.get(i..i + 2).unwrap_or_default().iter().collect();
            let sym = match two.as_str() {
                "<>" => Some("<>"),
                "!=" => Some("<>"),
                "<=" => Some("<="),
                ">=" => Some(">="),
                _ => None,
            };
            if let Some(s) = sym {
                out.push(Tok::Sym(s));
                i += 2;
                continue;
            }
            let one = match c {
                '=' => "=",
                '<' => "<",
                '>' => ">",
                '(' => "(",
                ')' => ")",
                ',' => ",",
                '+' => "+",
                '-' => "-",
                '*' => "*",
                '/' => "/",
                _ => return Err(Error::Query(format!("unexpected character `{c}`"))),
            };
            out.push(Tok::Sym(one));
            i += 1;
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    i: usize,
    fc: &'a FeatureClass,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    fn keyword(&mut self, k: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Ident(s)) if s.eq_ignore_ascii_case(k)) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn keyword_at(&self, offset: usize, k: &str) -> bool {
        matches!(self.toks.get(self.i + offset), Some(Tok::Ident(s)) if s.eq_ignore_ascii_case(k))
    }

    fn sym(&mut self, s: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Sym(t)) if *t == s) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, s: &str) -> Result<()> {
        if self.sym(s) {
            Ok(())
        } else {
            Err(Error::Query(match self.peek() {
                Some(t) => format!("expected `{s}`, found {}", show(t)),
                None => format!("expected `{s}` at the end"),
            }))
        }
    }

    fn deeper(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(Error::Query("the clause nests too deeply".into())) } else { Ok(()) }
    }

    fn or(&mut self) -> Result<Expr> {
        let mut left = self.and()?;
        while self.keyword("OR") {
            left = Expr::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr> {
        let mut left = self.not()?;
        while self.keyword("AND") {
            left = Expr::And(Box::new(left), Box::new(self.not()?));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr> {
        if self.keyword("NOT") {
            self.deeper()?;
            let e = self.not()?;
            self.depth -= 1;
            return Ok(Expr::Not(Box::new(e)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr> {
        let left = self.sum()?;
        let op = match self.peek() {
            Some(Tok::Sym("=")) => Some(Op::Eq),
            Some(Tok::Sym("<>")) => Some(Op::Ne),
            Some(Tok::Sym("<")) => Some(Op::Lt),
            Some(Tok::Sym("<=")) => Some(Op::Le),
            Some(Tok::Sym(">")) => Some(Op::Gt),
            Some(Tok::Sym(">=")) => Some(Op::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.i += 1;
            return Ok(Expr::Cmp(Box::new(left), op, Box::new(self.sum()?)));
        }
        if self.keyword("IS") {
            let negated = self.keyword("NOT");
            if !self.keyword("NULL") {
                return Err(Error::Query("expected NULL after IS".into()));
            }
            return Ok(Expr::IsNull(Box::new(left), negated));
        }
        let negated = (self.keyword_at(0, "NOT") && (self.keyword_at(1, "LIKE") || self.keyword_at(1, "IN") || self.keyword_at(1, "BETWEEN")))
            && self.keyword("NOT");
        if self.keyword("LIKE") {
            return Ok(Expr::Like(Box::new(left), Box::new(self.sum()?), negated));
        }
        if self.keyword("IN") {
            self.expect("(")?;
            let mut items = vec![self.sum()?];
            while self.sym(",") {
                items.push(self.sum()?);
            }
            self.expect(")")?;
            return Ok(Expr::In(Box::new(left), items, negated));
        }
        if self.keyword("BETWEEN") {
            let low = self.sum()?;
            if !self.keyword("AND") {
                return Err(Error::Query("expected AND in BETWEEN".into()));
            }
            return Ok(Expr::Between(Box::new(left), Box::new(low), Box::new(self.sum()?), negated));
        }
        Ok(left)
    }

    fn sum(&mut self) -> Result<Expr> {
        let mut left = self.term()?;
        loop {
            let op = if self.sym("+") {
                '+'
            } else if self.sym("-") {
                '-'
            } else {
                return Ok(left);
            };
            left = Expr::Arith(Box::new(left), op, Box::new(self.term()?));
        }
    }

    fn term(&mut self) -> Result<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = if self.sym("*") {
                '*'
            } else if self.sym("/") {
                '/'
            } else {
                return Ok(left);
            };
            left = Expr::Arith(Box::new(left), op, Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr> {
        if self.sym("-") {
            self.deeper()?;
            let e = self.unary()?;
            self.depth -= 1;
            return Ok(Expr::Neg(Box::new(e)));
        }
        self.primary()
    }

    fn field(&self, name: &str) -> Result<Expr> {
        self.fc.field_index(name).map(Expr::Field).ok_or_else(|| Error::Query(format!("no field named `{name}`")))
    }

    fn primary(&mut self) -> Result<Expr> {
        let tok = self.peek().cloned().ok_or_else(|| Error::Query("the clause ends too early".into()))?;
        self.i += 1;
        match tok {
            Tok::Num(n) => Ok(Expr::Lit(Value::Float(n))),
            Tok::Str(s) => Ok(Expr::Lit(Value::Text(s))),
            Tok::Quoted(name) => self.field(&name),
            Tok::Sym("(") => {
                self.deeper()?;
                let e = self.or()?;
                self.depth -= 1;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Ident(word) => {
                let upper = word.to_ascii_uppercase();
                match upper.as_str() {
                    "NULL" => Ok(Expr::Lit(Value::Null)),
                    "TRUE" => Ok(Expr::Lit(Value::Bool(true))),
                    "FALSE" => Ok(Expr::Lit(Value::Bool(false))),
                    "UPPER" | "LOWER" if matches!(self.peek(), Some(Tok::Sym("("))) => {
                        self.i += 1;
                        self.deeper()?;
                        let e = self.or()?;
                        self.depth -= 1;
                        self.expect(")")?;
                        Ok(if upper == "UPPER" { Expr::Upper(Box::new(e)) } else { Expr::Lower(Box::new(e)) })
                    }
                    _ => self.field(&word),
                }
            }
            Tok::Sym(s) => Err(Error::Query(format!("unexpected `{s}`"))),
        }
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        other => other.as_f64().is_some_and(|x| x != 0.0),
    }
}

/// Compares two non-null values: numerically when both read as numbers, else as text
/// (case-sensitive, as file geodatabases compare).
fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    if a.is_null() || b.is_null() {
        return None;
    }
    let numeric = |v: &Value| matches!(v, Value::Int(_) | Value::Float(_) | Value::Bool(_));
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) if numeric(a) || numeric(b) => x.partial_cmp(&y),
        _ => Some(a.to_string().cmp(&b.to_string())),
    }
}

/// SQL `LIKE`: `%` any run, `_` one character. Iterative with backtracking to the last `%`.
fn like(text: &str, pattern: &str) -> bool {
    let (t, p): (Vec<char>, Vec<char>) = (text.chars().collect(), pattern.chars().collect());
    let (mut ti, mut pi) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some('%') => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(c) if *c == '_' || Some(c) == t.get(ti) => {
                ti += 1;
                pi += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..).unwrap_or_default().iter().all(|c| *c == '%')
}

fn eval(e: &Expr, fc: &FeatureClass, row: usize) -> Value {
    let b = Value::Bool;
    match e {
        Expr::Field(i) => fc.value(row, *i).cloned().unwrap_or_default(),
        Expr::Lit(v) => v.clone(),
        Expr::Neg(x) => eval(x, fc, row).as_f64().map_or(Value::Null, |v| Value::Float(-v)),
        Expr::Not(x) => b(!truthy(&eval(x, fc, row))),
        Expr::And(x, y) => b(truthy(&eval(x, fc, row)) && truthy(&eval(y, fc, row))),
        Expr::Or(x, y) => b(truthy(&eval(x, fc, row)) || truthy(&eval(y, fc, row))),
        Expr::Cmp(x, op, y) => {
            let Some(ord) = compare(&eval(x, fc, row), &eval(y, fc, row)) else { return b(false) };
            use std::cmp::Ordering::*;
            b(match op {
                Op::Eq => ord == Equal,
                Op::Ne => ord != Equal,
                Op::Lt => ord == Less,
                Op::Le => ord != Greater,
                Op::Gt => ord == Greater,
                Op::Ge => ord != Less,
            })
        }
        Expr::Arith(x, op, y) => {
            let (Some(p), Some(q)) = (eval(x, fc, row).as_f64(), eval(y, fc, row).as_f64()) else { return Value::Null };
            let r = match op {
                '+' => p + q,
                '-' => p - q,
                '*' => p * q,
                _ if q == 0.0 => return Value::Null,
                _ => p / q,
            };
            if r.is_finite() { Value::Float(r) } else { Value::Null }
        }
        Expr::IsNull(x, negated) => b(eval(x, fc, row).is_null() != *negated),
        Expr::Like(x, pat, negated) => {
            let (v, p) = (eval(x, fc, row), eval(pat, fc, row));
            if v.is_null() || p.is_null() {
                return b(false);
            }
            b(like(&v.to_string(), &p.to_string()) != *negated)
        }
        Expr::In(x, items, negated) => {
            let v = eval(x, fc, row);
            if v.is_null() {
                return b(false);
            }
            let found = items.iter().any(|i| compare(&v, &eval(i, fc, row)) == Some(std::cmp::Ordering::Equal));
            b(found != *negated)
        }
        Expr::Between(x, lo, hi, negated) => {
            let v = eval(x, fc, row);
            let (Some(a), Some(c)) = (compare(&v, &eval(lo, fc, row)), compare(&v, &eval(hi, fc, row))) else { return b(false) };
            b((a != std::cmp::Ordering::Less && c != std::cmp::Ordering::Greater) != *negated)
        }
        Expr::Upper(x) => match eval(x, fc, row) {
            Value::Null => Value::Null,
            v => Value::Text(v.to_string().to_uppercase()),
        },
        Expr::Lower(x) => match eval(x, fc, row) {
            Value::Null => Value::Null,
            v => Value::Text(v.to_string().to_lowercase()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Field, FieldType};
    use aegis_crs::Crs;
    use aegis_geom::GeometryType;

    fn table() -> FeatureClass {
        let mut fc = FeatureClass::new(
            "t",
            GeometryType::Point,
            Crs::WGS84,
            vec![Field::new("NAME", FieldType::Text), Field::new("POP", FieldType::Integer), Field::new("Tipo Area", FieldType::Text)],
        );
        let rows = [("São Paulo", Some(12_000_000), "metro"), ("Santos", Some(430_000), "porto"), ("Ilha", None, "metro")];
        for (n, p, t) in rows {
            fc.push(None, vec![Value::Text(n.into()), p.map_or(Value::Null, Value::Int), Value::Text(t.into())]).unwrap();
        }
        fc
    }

    fn sel(q: &str) -> Vec<usize> {
        let fc = table();
        Query::parse(q, &fc).unwrap().select(&fc)
    }

    #[test]
    fn where_clauses() {
        assert_eq!(sel("POP > 1000000"), vec![0]);
        assert_eq!(sel("pop >= 430000 and name like 'S%'"), vec![0, 1]);
        assert_eq!(sel("NAME LIKE 'S_ntos'"), vec![1]);
        assert_eq!(sel("POP IS NULL"), vec![2]);
        assert_eq!(sel("NOT POP > 1000000"), vec![1, 2]);
        assert_eq!(sel("\"Tipo Area\" IN ('porto', 'x')"), vec![1]);
        assert_eq!(sel("\"Tipo Area\" NOT IN ('porto')"), vec![0, 2]);
        assert_eq!(sel("POP BETWEEN 1 AND 500000"), vec![1]);
        assert_eq!(sel("POP / 1000 = 430"), vec![1]);
        assert_eq!(sel("UPPER(NAME) = 'SANTOS' OR POP <> POP"), vec![1]);
        assert_eq!(sel("POP / 0 = 1"), Vec::<usize>::new());
        assert_eq!(sel("NAME = 'it''s'"), Vec::<usize>::new());
        assert_eq!(sel("(((POP > 0)))"), vec![0, 1]);
        assert_eq!(sel("-POP < -1000000"), vec![0]);
        let fc = table();
        assert_eq!(Query::parse("POP / 1000", &fc).unwrap().evaluate(&fc, 1), Value::Float(430.0));
        assert_eq!(Query::parse("UPPER(NAME)", &fc).unwrap().evaluate(&fc, 1), Value::Text("SANTOS".into()));
    }

    #[test]
    fn bad_clauses_are_errors() {
        let fc = table();
        for q in ["", "POP >", "NOPE = 1", "NAME = 'open", "POP IS 3", "(POP > 1", "POP > 1 POP", "POP @ 2", "x BETWEEN 1"] {
            assert!(Query::parse(q, &fc).is_err(), "{q}");
        }
        let deep = format!("{}POP > 1{}", "(".repeat(10_000), ")".repeat(10_000));
        assert!(Query::parse(&deep, &fc).is_err());
        let nots = format!("{} POP > 1", "NOT ".repeat(10_000));
        assert!(Query::parse(&nots, &fc).is_err());
    }

    #[test]
    fn like_backtracks() {
        assert!(like("abcabc", "%abc"));
        assert!(like("abc", "a%c"));
        assert!(!like("abc", "a%d"));
        assert!(like("", "%"));
        assert!(like(&"a".repeat(5000), &"%a".repeat(50)));
    }
}
