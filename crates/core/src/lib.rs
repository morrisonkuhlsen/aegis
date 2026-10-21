//! mkaeGIS's foundation (L0): the error type every crate returns, attribute values and colours.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fmt;

use serde::{Deserialize, Serialize};

/// The most items an input may make us allocate at once (features, vertices, rows, pixels):
/// hostile files and parameters declare huge counts.
pub const MAX_ITEMS: usize = 50_000_000;

/// Every error mkaeGIS reports. Messages are for people: they say what went wrong and where.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("invalid parameter `{name}`: {reason}")]
    Param { name: String, reason: String },
    #[error("{0} not found")]
    NotFound(String),
    #[error("cannot read {what}: {reason}")]
    Format { what: String, reason: String },
    #[error("{0}")]
    Io(String),
    #[error("geometry: {0}")]
    Geometry(String),
    #[error("projection: {0}")]
    Projection(String),
    #[error("query: {0}")]
    Query(String),
    #[error("{0} is not available yet")]
    NotAvailable(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    pub fn param(name: &str, reason: impl Into<String>) -> Self {
        Error::Param { name: name.to_string(), reason: reason.into() }
    }

    pub fn format(what: &str, reason: impl Into<String>) -> Self {
        Error::Format { what: what.to_string(), reason: reason.into() }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A field's value in an attribute table row.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl Value {
    /// The value as a number, when it is one (or text that reads as one).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) if f.is_finite() => Some(*f),
            Value::Text(t) => t.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "<Null>"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => {
                if x.fract() == 0.0 && x.abs() < 1e15 {
                    write!(f, "{x:.0}")
                } else {
                    write!(f, "{}", (x * 1e6).round() / 1e6)
                }
            }
            Value::Text(t) => write!(f, "{t}"),
        }
    }
}

/// An sRGB colour with straight alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// The selection highlight.
    pub const SELECTION: Color = Color::rgb(0, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }

    /// `#rrggbb` or `#rrggbbaa`.
    pub fn from_hex(text: &str) -> Option<Color> {
        let hex = text.trim().strip_prefix('#').unwrap_or(text.trim());
        let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex.len() {
            6 => Some(Color::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Color { a, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let c = Color::rgba(1, 2, 250, 128);
        assert_eq!(Color::from_hex(&c.to_hex()), Some(c));
        assert_eq!(Color::from_hex("#00ff00"), Some(Color::rgb(0, 255, 0)));
        assert_eq!(Color::from_hex("#zz0000"), None);
        assert_eq!(Color::from_hex("#é0000"), None);
    }

    #[test]
    fn values_read_as_numbers() {
        assert_eq!(Value::Text(" 12.5 ".into()).as_f64(), Some(12.5));
        assert_eq!(Value::Float(f64::NAN).as_f64(), None);
        assert_eq!(Value::Null.as_f64(), None);
        assert_eq!(Value::Float(3.0).to_string(), "3");
    }
}
