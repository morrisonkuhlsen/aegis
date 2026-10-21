//! Esri Shapefile in and out (L2), from the public *ESRI Shapefile Technical Description*
//! (July 1998) and the dBASE III table format: `.shp` shapes, `.shx` index, `.dbf` attributes,
//! `.prj` coordinate system (WKT) and `.cpg` code page.
//!
//! Read: null, point, multipoint, polyline and polygon shapes, and their Z and M variants (Z and
//! M dropped); MultiPatch is refused. dBASE text is UTF-8 unless the `.cpg` names another code
//! page, which is read as Latin-1. Written files are UTF-8 with a `.cpg` saying so; polygon shells
//! are written clockwise and holes counter-clockwise, as the specification requires.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod dbf;

use std::path::Path;

use mkaegis_core::{Error, Result, Value};
use mkaegis_crs::Crs;
use mkaegis_geom::{Coord, Envelope, Geometry, GeometryType};
use mkaegis_model::FeatureClass;

const FILE_CODE: i32 = 9994;
const VERSION: i32 = 1000;
const HEADER: usize = 100;

/// A shapefile's files in memory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Files {
    pub shp: Vec<u8>,
    pub shx: Vec<u8>,
    pub dbf: Vec<u8>,
    pub prj: Option<String>,
    pub cpg: Option<String>,
}

/// A feature class read from a shapefile, with anything the reader had to work around.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub data: FeatureClass,
    pub warnings: Vec<String>,
}

struct Bytes<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let end = self.at.checked_add(N).ok_or_else(|| Error::format("shapefile", "offset overflow"))?;
        let s = self.b.get(self.at..end).ok_or_else(|| Error::format("shapefile", format!("truncated at byte {}", self.at)))?;
        self.at = end;
        let mut out = [0u8; N];
        out.copy_from_slice(s);
        Ok(out)
    }

    fn be_i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take()?))
    }

    fn le_i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take()?))
    }

    fn le_f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take()?))
    }

    /// A count from the file, checked against the bytes left (each item at least `item` bytes).
    fn count(&mut self, item: usize) -> Result<usize> {
        let n = self.le_i32()?;
        let n = usize::try_from(n).map_err(|_| Error::format("shapefile", format!("negative count {n}")))?;
        if n.saturating_mul(item) > self.b.len().saturating_sub(self.at) {
            return Err(Error::format("shapefile", format!("count {n} exceeds the record")));
        }
        Ok(n)
    }

    fn point(&mut self) -> Result<Coord> {
        let (x, y) = (self.le_f64()?, self.le_f64()?);
        if !(x.is_finite() && y.is_finite()) {
            return Err(Error::format("shapefile", "non-finite coordinate"));
        }
        Ok(Coord::new(x, y))
    }
}

fn shape_family(code: i32) -> Option<GeometryType> {
    match code {
        1 | 11 | 21 => Some(GeometryType::Point),
        8 | 18 | 28 => Some(GeometryType::Multipoint),
        3 | 13 | 23 => Some(GeometryType::Polyline),
        5 | 15 | 25 => Some(GeometryType::Polygon),
        _ => None,
    }
}

/// One record's shape (`None` for a null shape).
fn read_shape(content: &[u8]) -> Result<Option<Geometry>> {
    let mut r = Bytes { b: content, at: 0 };
    let code = r.le_i32()?;
    if code == 0 {
        return Ok(None);
    }
    let family = shape_family(code).ok_or_else(|| Error::NotAvailable(format!("shape type {code}")))?;
    Ok(Some(match family {
        GeometryType::Point => Geometry::Point(r.point()?),
        GeometryType::Multipoint => {
            r.at += 32;
            let n = r.count(16)?;
            Geometry::Multipoint((0..n).map(|_| r.point()).collect::<Result<_>>()?)
        }
        GeometryType::Polyline | GeometryType::Polygon => {
            r.at += 32;
            let parts = r.count(4)?;
            let points = r.count(16)?;
            let starts: Vec<usize> = (0..parts).map(|_| r.le_i32().map(|s| usize::try_from(s).unwrap_or(usize::MAX))).collect::<Result<_>>()?;
            let pts: Vec<Coord> = (0..points).map(|_| r.point()).collect::<Result<_>>()?;
            let mut paths = Vec::with_capacity(parts);
            for (i, &s) in starts.iter().enumerate() {
                let e = starts.get(i + 1).copied().unwrap_or(points);
                let path = pts.get(s..e).ok_or_else(|| Error::format("shapefile", "part index out of range"))?;
                paths.push(path.to_vec());
            }
            if family == GeometryType::Polyline { Geometry::Polyline(paths) } else { Geometry::polygon(paths)? }
        }
    }))
}

/// Reads a shapefile from its files in memory.
pub fn read(files: &Files, name: &str) -> Result<Read> {
    let mut warnings = Vec::new();
    let mut h = Bytes { b: &files.shp, at: 0 };
    if h.be_i32()? != FILE_CODE {
        return Err(Error::format("shapefile", "not a .shp file (bad file code)"));
    }
    h.at = 28;
    let version = h.le_i32()?;
    if version != VERSION {
        warnings.push(format!("unexpected version {version}"));
    }
    let code = h.le_i32()?;
    let geometry_type = match code {
        0 => GeometryType::Point,
        c => shape_family(c).ok_or_else(|| Error::NotAvailable(format!("shape type {c}")))?,
    };

    let table = dbf::read(&files.dbf, files.cpg.as_deref())?;
    let crs = match files.prj.as_deref().map(Crs::from_wkt) {
        Some(Some(crs)) => crs,
        prj => {
            warnings.push(if prj.is_none() { "no .prj: assumed WGS 84".into() } else { "unrecognised .prj: assumed WGS 84".into() });
            Crs::WGS84
        }
    };
    let mut data = FeatureClass::new(name, geometry_type, crs, table.fields);

    let mut r = Bytes { b: &files.shp, at: HEADER };
    let mut rows = table.rows.into_iter();
    let mut skipped = 0usize;
    while r.at + 8 <= files.shp.len() {
        let _number = r.be_i32()?;
        let words = usize::try_from(r.be_i32()?).map_err(|_| Error::format("shapefile", "negative record length"))?;
        let len = words.checked_mul(2).ok_or_else(|| Error::format("shapefile", "record length overflow"))?;
        let end = r.at.checked_add(len).filter(|e| *e <= files.shp.len()).ok_or_else(|| Error::format("shapefile", "record runs past the end"))?;
        let content = files.shp.get(r.at..end).unwrap_or_default();
        r.at = end;
        let values = rows.next().unwrap_or_default();
        match read_shape(content) {
            Ok(g) => data.push(g, values)?,
            Err(e) => {
                skipped += 1;
                if skipped <= 5 {
                    warnings.push(format!("record {} skipped: {e}", data.features.len() + skipped));
                }
            }
        }
    }
    if skipped > 5 {
        warnings.push(format!("{skipped} records skipped in all"));
    }
    if rows.next().is_some() {
        warnings.push("the .dbf has more rows than the .shp has shapes".into());
    }
    if files.prj.is_none() && data.extent().is_some_and(|e| e.min_x < -180.5 || e.max_x > 360.5 || e.min_y < -90.5 || e.max_y > 90.5) {
        return Err(Error::format("shapefile", "no .prj, and the coordinates are not longitude/latitude: the coordinate system is unknown"));
    }
    Ok(Read { data, warnings })
}

/// Reads `name.shp` and its sidecar files (same stem, any case of extension).
pub fn read_path(path: &Path) -> Result<Read> {
    let sidecar = |ext: &str| -> Option<Vec<u8>> {
        [ext.to_ascii_lowercase(), ext.to_ascii_uppercase()].iter().find_map(|e| std::fs::read(path.with_extension(e)).ok())
    };
    let shp = std::fs::read(path)?;
    let files = Files {
        shp,
        shx: sidecar("shx").unwrap_or_default(),
        dbf: sidecar("dbf").ok_or_else(|| Error::NotFound(format!("{}", path.with_extension("dbf").display())))?,
        prj: sidecar("prj").map(|b| String::from_utf8_lossy(&b).into_owned()),
        cpg: sidecar("cpg").map(|b| String::from_utf8_lossy(&b).into_owned()),
    };
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "shapefile".into());
    read(&files, &name)
}

fn signed_area2(ring: &[Coord]) -> f64 {
    ring.windows(2).map(|w| if let [a, b] = w { a.x * b.y - b.x * a.y } else { 0.0 }).sum()
}

/// One shape's record content.
fn write_shape(g: Option<&Geometry>, out: &mut Vec<u8>) {
    let push_f = |out: &mut Vec<u8>, v: f64| out.extend_from_slice(&v.to_le_bytes());
    let push_i = |out: &mut Vec<u8>, v: i32| out.extend_from_slice(&v.to_le_bytes());
    let bbox = |out: &mut Vec<u8>, e: Option<Envelope>| {
        let e = e.unwrap_or(Envelope { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0 });
        for v in [e.min_x, e.min_y, e.max_x, e.max_y] {
            push_f(out, v);
        }
    };
    let count = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
    match g {
        None => push_i(out, 0),
        Some(Geometry::Point(c)) => {
            push_i(out, 1);
            push_f(out, c.x);
            push_f(out, c.y);
        }
        Some(g @ Geometry::Multipoint(cs)) => {
            push_i(out, 8);
            bbox(out, g.envelope());
            push_i(out, count(cs.len()));
            for c in cs {
                push_f(out, c.x);
                push_f(out, c.y);
            }
        }
        Some(g @ (Geometry::Polyline(parts) | Geometry::Polygon(parts))) => {
            let polygon = matches!(g, Geometry::Polygon(_));
            // Shells clockwise, holes counter-clockwise.
            let parts: Vec<Vec<Coord>> = if polygon {
                mkaegis_geom::wkt::shells_with_holes(parts)
                    .into_iter()
                    .flat_map(|group| {
                        group.into_iter().enumerate().map(|(i, r)| {
                            let mut r = r.clone();
                            if (signed_area2(&r) < 0.0) != (i == 0) {
                                r.reverse();
                            }
                            r
                        })
                    })
                    .collect()
            } else {
                parts.clone()
            };
            push_i(out, if polygon { 5 } else { 3 });
            bbox(out, g.envelope());
            push_i(out, count(parts.len()));
            push_i(out, count(parts.iter().map(Vec::len).sum()));
            let mut start = 0usize;
            for p in &parts {
                push_i(out, count(start));
                start += p.len();
            }
            for c in parts.iter().flatten() {
                push_f(out, c.x);
                push_f(out, c.y);
            }
        }
    }
}

fn header(out: &mut Vec<u8>, length_bytes: usize, code: i32, e: Option<Envelope>) {
    out.extend_from_slice(&FILE_CODE.to_be_bytes());
    out.extend_from_slice(&[0u8; 20]);
    out.extend_from_slice(&i32::try_from(length_bytes / 2).unwrap_or(i32::MAX).to_be_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&code.to_le_bytes());
    let e = e.unwrap_or(Envelope { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0 });
    for v in [e.min_x, e.min_y, e.max_x, e.max_y, 0.0, 0.0, 0.0, 0.0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// Writes a feature class as a shapefile's files.
pub fn write(fc: &FeatureClass) -> Result<Files> {
    let code = match fc.geometry_type {
        GeometryType::Point => 1,
        GeometryType::Multipoint => 8,
        GeometryType::Polyline => 3,
        GeometryType::Polygon => 5,
    };
    let mut records = Vec::new();
    let mut index = Vec::new();
    for (i, f) in fc.features.iter().enumerate() {
        let mut content = Vec::new();
        write_shape(f.geometry.as_ref(), &mut content);
        let offset = HEADER + records.len();
        if offset / 2 > i32::MAX as usize {
            return Err(Error::param("features", "the .shp would pass the format's 4 GB limit"));
        }
        records.extend_from_slice(&i32::try_from(i + 1).unwrap_or(i32::MAX).to_be_bytes());
        records.extend_from_slice(&i32::try_from(content.len() / 2).unwrap_or(i32::MAX).to_be_bytes());
        index.extend_from_slice(&i32::try_from(offset / 2).unwrap_or(i32::MAX).to_be_bytes());
        index.extend_from_slice(&i32::try_from(content.len() / 2).unwrap_or(i32::MAX).to_be_bytes());
        records.extend_from_slice(&content);
    }
    let extent = fc.extent();
    let mut shp = Vec::with_capacity(HEADER + records.len());
    header(&mut shp, HEADER + records.len(), code, extent);
    shp.extend_from_slice(&records);
    let mut shx = Vec::with_capacity(HEADER + index.len());
    header(&mut shx, HEADER + index.len(), code, extent);
    shx.extend_from_slice(&index);
    let rows: Vec<&[Value]> = fc.features.iter().map(|f| f.values.as_slice()).collect();
    Ok(Files { shp, shx, dbf: dbf::write(&fc.fields, &rows)?, prj: Some(fc.crs.to_prj()), cpg: Some("UTF-8".into()) })
}

/// Writes `path` (`.shp`) and its sidecar files.
pub fn write_path(fc: &FeatureClass, path: &Path) -> Result<()> {
    let files = write(fc)?;
    std::fs::write(path.with_extension("shp"), &files.shp)?;
    std::fs::write(path.with_extension("shx"), &files.shx)?;
    std::fs::write(path.with_extension("dbf"), &files.dbf)?;
    if let Some(prj) = &files.prj {
        std::fs::write(path.with_extension("prj"), prj)?;
    }
    if let Some(cpg) = &files.cpg {
        std::fs::write(path.with_extension("cpg"), cpg)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkaegis_model::{Field, FieldType};

    fn square(x: f64, y: f64, s: f64) -> Vec<Coord> {
        vec![Coord::new(x, y), Coord::new(x + s, y), Coord::new(x + s, y + s), Coord::new(x, y + s), Coord::new(x, y)]
    }

    fn polygons() -> FeatureClass {
        let mut fc = FeatureClass::new(
            "lots",
            GeometryType::Polygon,
            Crs::from_epsg(31983).unwrap(),
            vec![
                Field::new("NOME", FieldType::Text),
                Field::new("AREA_HA", FieldType::Double),
                Field::new("CODIGO", FieldType::Integer),
                Field::new("ATIVO", FieldType::Boolean),
            ],
        );
        let g = Geometry::Polygon(vec![square(300_000.0, 7_400_000.0, 100.0), square(300_010.0, 7_400_010.0, 10.0)]);
        fc.push(Some(g), vec![Value::Text("Lote São João".into()), Value::Float(0.99), Value::Int(-42), Value::Bool(true)]).unwrap();
        fc.push(None, vec![Value::Null, Value::Null, Value::Null, Value::Null]).unwrap();
        fc.push(Some(Geometry::Polygon(vec![square(0.0, 0.0, 1.0)])), vec![Value::Text("x".into()), Value::Float(12_345_678.25), Value::Int(7)])
            .unwrap();
        fc
    }

    #[test]
    fn round_trips() {
        let fc = polygons();
        let files = write(&fc).unwrap();
        assert_eq!(files.shp.len() % 2, 0);
        assert_eq!(files.shx.len(), HEADER + 8 * 3);
        let back = read(&files, "lots").unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        assert_eq!(back.data.crs, fc.crs);
        assert_eq!(back.data.features.len(), 3);
        assert_eq!(back.data.features[1].geometry, None);
        let a = |f: &FeatureClass, i: usize| mkaegis_geom::algo::area(f.features[i].geometry.as_ref().unwrap());
        assert_eq!(a(&back.data, 0), a(&fc, 0));
        assert_eq!(back.data.features[0].values, fc.features[0].values);
        assert_eq!(back.data.features[1].values, fc.features[1].values);
        assert_eq!(back.data.features[2].values[1], Value::Float(12_345_678.25));
        // The shell is written clockwise.
        let Some(Geometry::Polygon(rings)) = &back.data.features[0].geometry else { panic!("no polygon") };
        assert!(signed_area2(&rings[0]) < 0.0 && signed_area2(&rings[1]) > 0.0);
    }

    #[test]
    fn points_and_lines_round_trip() {
        let mut pts = FeatureClass::new("p", GeometryType::Point, Crs::WGS84, vec![]);
        pts.push(Some(Geometry::Point(Coord::new(-46.6, -23.5))), vec![]).unwrap();
        let back = read(&write(&pts).unwrap(), "p").unwrap();
        assert_eq!(back.data.features[0].geometry, pts.features[0].geometry);
        let mut lines = FeatureClass::new("l", GeometryType::Polyline, Crs::WGS84, vec![]);
        let g = Geometry::Polyline(vec![vec![Coord::new(0.0, 0.0), Coord::new(1.0, 1.0)], vec![Coord::new(2.0, 2.0), Coord::new(3.0, 1.0)]]);
        lines.push(Some(g.clone()), vec![]).unwrap();
        assert_eq!(read(&write(&lines).unwrap(), "l").unwrap().data.features[0].geometry, Some(g));
    }

    #[test]
    fn hostile_files_never_panic() {
        let good = write(&polygons()).unwrap();
        // Every truncation and a byte flipped at every position: errors or warnings, no panic.
        for cut in 0..good.shp.len() {
            let files = Files { shp: good.shp.get(..cut).unwrap().to_vec(), ..good.clone() };
            let _ = read(&files, "x");
        }
        for i in 0..good.shp.len() {
            let mut files = good.clone();
            files.shp[i] ^= 0xff;
            let _ = read(&files, "x");
        }
        for i in 0..good.dbf.len() {
            let mut files = good.clone();
            files.dbf[i] ^= 0xff;
            let _ = read(&files, "x");
        }
        assert!(read(&Files::default(), "x").is_err());
    }

    #[test]
    fn missing_prj_on_projected_data_is_an_error() {
        let mut files = write(&polygons()).unwrap();
        files.prj = None;
        assert!(read(&files, "x").is_err());
    }
}
