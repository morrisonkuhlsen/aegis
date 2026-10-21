//! Coordinate reference systems (L0), identified by EPSG code: geographic WGS 84 and SIRGAS 2000,
//! WGS 84 / Pseudo-Mercator (the web maps' projection), and UTM zones on WGS 84 and SIRGAS 2000.
//!
//! The math is public: the spherical Mercator, and Krüger's series for the transverse Mercator
//! (to n⁴, sub-millimetre within a UTM zone), as given in USGS Professional Paper 1395 (Snyder,
//! *Map Projections: A Working Manual*) and Karney (2011). WGS 84 and SIRGAS 2000 are treated as
//! the same datum (they agree within centimetres); no datum shifts yet.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::f64::consts::{FRAC_PI_4, PI};

use mkaegis_core::{Error, Result};
use mkaegis_geom::Coord;
use serde::{Deserialize, Serialize};

/// Web Mercator's sphere radius: WGS 84's semi-major axis.
const WEB_MERCATOR_R: f64 = 6_378_137.0;
/// Web Mercator stops here: the latitude at which the world is a square.
pub const WEB_MERCATOR_MAX_LAT: f64 = 85.051_128_779_806_59;
const UTM_K0: f64 = 0.9996;
const UTM_FALSE_EASTING: f64 = 500_000.0;
const UTM_FALSE_NORTHING_SOUTH: f64 = 10_000_000.0;

/// A reference ellipsoid: semi-major axis and inverse flattening.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipsoid {
    pub name: &'static str,
    pub a: f64,
    pub inv_f: f64,
}

pub const WGS84: Ellipsoid = Ellipsoid { name: "WGS_1984", a: 6_378_137.0, inv_f: 298.257_223_563 };
pub const GRS80: Ellipsoid = Ellipsoid { name: "GRS_1980", a: 6_378_137.0, inv_f: 298.257_222_101 };

/// A coordinate reference system, by its EPSG code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct Crs(u32);

impl TryFrom<u32> for Crs {
    type Error = Error;
    fn try_from(code: u32) -> Result<Crs> {
        Crs::from_epsg(code)
    }
}

impl From<Crs> for u32 {
    fn from(crs: Crs) -> u32 {
        crs.0
    }
}

/// What a CRS is, resolved from its code.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Geographic { datum: Datum },
    WebMercator,
    Utm { datum: Datum, zone: u8, south: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Datum {
    Wgs84,
    Sirgas2000,
}

impl Datum {
    fn ellipsoid(self) -> Ellipsoid {
        match self {
            Datum::Wgs84 => WGS84,
            Datum::Sirgas2000 => GRS80,
        }
    }

    fn esri(self) -> (&'static str, &'static str) {
        match self {
            Datum::Wgs84 => ("GCS_WGS_1984", "D_WGS_1984"),
            Datum::Sirgas2000 => ("GCS_SIRGAS_2000", "D_SIRGAS_2000"),
        }
    }
}

impl Crs {
    pub const WGS84: Crs = Crs(4326);
    pub const SIRGAS2000: Crs = Crs(4674);
    pub const WEB_MERCATOR: Crs = Crs(3857);

    /// The CRS with this EPSG code, if mkaeGIS knows it.
    pub fn from_epsg(code: u32) -> Result<Crs> {
        match code {
            4326 | 4674 | 3857 | 32601..=32660 | 32701..=32760 | 31978..=31985 => Ok(Crs(code)),
            // Old aliases of Web Mercator.
            900_913 | 102_100 | 3785 => Ok(Crs::WEB_MERCATOR),
            _ => Err(Error::Projection(format!("unknown or unsupported EPSG code {code}"))),
        }
    }

    /// The WGS 84 UTM zone that holds this longitude/latitude.
    pub fn utm_for(lon: f64, lat: f64) -> Result<Crs> {
        if !(lon.is_finite() && lat.is_finite()) || lat.abs() > 84.0 {
            return Err(Error::Projection("UTM covers latitudes 80°S to 84°N".into()));
        }
        let zone = (((lon + 180.0).rem_euclid(360.0) / 6.0).floor() as u32 + 1).min(60);
        Crs::from_epsg(if lat < 0.0 { 32700 } else { 32600 } + zone)
    }

    pub fn epsg(self) -> u32 {
        self.0
    }

    pub fn kind(self) -> Kind {
        match self.0 {
            4674 => Kind::Geographic { datum: Datum::Sirgas2000 },
            3857 => Kind::WebMercator,
            c @ 32601..=32660 => Kind::Utm { datum: Datum::Wgs84, zone: (c - 32600) as u8, south: false },
            c @ 32701..=32760 => Kind::Utm { datum: Datum::Wgs84, zone: (c - 32700) as u8, south: true },
            c @ 31978..=31985 => Kind::Utm { datum: Datum::Sirgas2000, zone: (c - 31960) as u8, south: true },
            _ => Kind::Geographic { datum: Datum::Wgs84 },
        }
    }

    pub fn name(self) -> String {
        match self.kind() {
            Kind::Geographic { datum: Datum::Wgs84 } => "WGS 84".into(),
            Kind::Geographic { datum: Datum::Sirgas2000 } => "SIRGAS 2000".into(),
            Kind::WebMercator => "WGS 84 / Pseudo-Mercator".into(),
            Kind::Utm { datum, zone, south } => {
                format!("{} / UTM zone {zone}{}", if datum == Datum::Wgs84 { "WGS 84" } else { "SIRGAS 2000" }, if south { 'S' } else { 'N' })
            }
        }
    }

    pub fn is_geographic(self) -> bool {
        matches!(self.kind(), Kind::Geographic { .. })
    }

    /// The unit coordinates are in: "degree" or "metre".
    pub fn unit(self) -> &'static str {
        if self.is_geographic() { "degree" } else { "metre" }
    }

    /// Metres in one coordinate unit near latitude `lat` (Web Mercator's scale grows with latitude).
    pub fn metres_per_unit(self, lat: f64) -> f64 {
        let lat = lat.clamp(-89.0, 89.0).to_radians();
        match self.kind() {
            Kind::Geographic { .. } => WGS84.a * PI / 180.0 * lat.cos().max(1e-6),
            Kind::WebMercator => lat.cos(),
            Kind::Utm { .. } => 1.0,
        }
    }

    /// Longitude/latitude (degrees) of a point in this CRS.
    pub fn to_geographic(self, c: Coord) -> Result<Coord> {
        if !c.is_finite() {
            return Err(Error::Projection("non-finite coordinate".into()));
        }
        match self.kind() {
            Kind::Geographic { .. } => Ok(c),
            Kind::WebMercator => {
                let lon = (c.x / WEB_MERCATOR_R).to_degrees();
                let lat = (2.0 * (c.y / WEB_MERCATOR_R).exp().atan() - PI / 2.0).to_degrees();
                Ok(Coord::new(lon, lat))
            }
            Kind::Utm { datum, zone, south } => {
                let tm = TransverseMercator::new(datum.ellipsoid());
                let y = c.y - if south { UTM_FALSE_NORTHING_SOUTH } else { 0.0 };
                let (lon, lat) = tm.inverse(c.x - UTM_FALSE_EASTING, y, central_meridian(zone));
                Ok(Coord::new(lon, lat))
            }
        }
    }

    /// A longitude/latitude point (degrees) in this CRS.
    pub fn from_geographic(self, g: Coord) -> Result<Coord> {
        if !g.is_finite() || g.y.abs() > 90.0 {
            return Err(Error::Projection(format!("not a longitude/latitude: {}, {}", g.x, g.y)));
        }
        match self.kind() {
            Kind::Geographic { .. } => Ok(g),
            Kind::WebMercator => {
                let lat = g.y.clamp(-WEB_MERCATOR_MAX_LAT, WEB_MERCATOR_MAX_LAT).to_radians();
                Ok(Coord::new(WEB_MERCATOR_R * g.x.to_radians(), WEB_MERCATOR_R * (FRAC_PI_4 + lat / 2.0).tan().ln()))
            }
            Kind::Utm { datum, zone, south } => {
                let cm = central_meridian(zone);
                let dlon = (g.x - cm + 540.0).rem_euclid(360.0) - 180.0;
                // Krüger's series diverges far from the central meridian.
                if dlon.abs() > 45.0 {
                    return Err(Error::Projection(format!("{}°, {}° is too far from UTM zone {zone}", g.x, g.y)));
                }
                let (e, n) = TransverseMercator::new(datum.ellipsoid()).forward(dlon, g.y);
                Ok(Coord::new(e + UTM_FALSE_EASTING, n + if south { UTM_FALSE_NORTHING_SOUTH } else { 0.0 }))
            }
        }
    }

    /// A point from this CRS into `to`.
    pub fn transform(self, to: Crs, c: Coord) -> Result<Coord> {
        if self == to {
            return Ok(c);
        }
        to.from_geographic(self.to_geographic(c)?)
    }

    /// The CRS as Esri-flavoured WKT, for a shapefile's `.prj`.
    pub fn to_prj(self) -> String {
        let gcs = |datum: Datum| {
            let (g, d) = datum.esri();
            let e = datum.ellipsoid();
            format!(
                "GEOGCS[\"{g}\",DATUM[\"{d}\",SPHEROID[\"{}\",{:.1},{}]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]]",
                e.name, e.a, e.inv_f
            )
        };
        match self.kind() {
            Kind::Geographic { datum } => gcs(datum),
            Kind::WebMercator => format!(
                "PROJCS[\"WGS_1984_Web_Mercator_Auxiliary_Sphere\",{},PROJECTION[\"Mercator_Auxiliary_Sphere\"],PARAMETER[\"False_Easting\",0.0],PARAMETER[\"False_Northing\",0.0],PARAMETER[\"Central_Meridian\",0.0],PARAMETER[\"Standard_Parallel_1\",0.0],PARAMETER[\"Auxiliary_Sphere_Type\",0.0],UNIT[\"Meter\",1.0]]",
                gcs(Datum::Wgs84)
            ),
            Kind::Utm { datum, zone, south } => format!(
                "PROJCS[\"{}_UTM_Zone_{zone}{}\",{},PROJECTION[\"Transverse_Mercator\"],PARAMETER[\"False_Easting\",500000.0],PARAMETER[\"False_Northing\",{:.1}],PARAMETER[\"Central_Meridian\",{:.1}],PARAMETER[\"Scale_Factor\",0.9996],PARAMETER[\"Latitude_Of_Origin\",0.0],UNIT[\"Meter\",1.0]]",
                if datum == Datum::Wgs84 { "WGS_1984" } else { "SIRGAS_2000" },
                if south { 'S' } else { 'N' },
                gcs(datum),
                if south { UTM_FALSE_NORTHING_SOUTH } else { 0.0 },
                central_meridian(zone)
            ),
        }
    }

    /// Recognises a CRS in WKT (a `.prj`, Esri or OGC flavour): by its EPSG authority, else by name.
    pub fn from_wkt(text: &str) -> Option<Crs> {
        // The outermost AUTHORITY comes last.
        if let Some(code) = text.rsplit("AUTHORITY[").next().filter(|_| text.contains("AUTHORITY[")).and_then(|tail| {
            let digits: String = tail.split(',').nth(1)?.chars().filter(char::is_ascii_digit).collect();
            digits.parse::<u32>().ok()
        }) && let Ok(crs) = Crs::from_epsg(code)
        {
            return Some(crs);
        }
        let name: String = text.split('"').nth(1)?.to_ascii_uppercase().replace([' ', '/'], "_").replace("__", "_").replace("__", "_");
        if name.contains("WEB_MERCATOR") || name.contains("PSEUDO_MERCATOR") {
            return Some(Crs::WEB_MERCATOR);
        }
        let sirgas = name.contains("SIRGAS");
        if let Some(rest) = name.split("UTM_ZONE_").nth(1) {
            let zone: u32 = rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()?;
            let south = rest.trim_start_matches(|c: char| c.is_ascii_digit()).starts_with('S');
            return Crs::from_epsg(match (sirgas, south) {
                (true, true) => 31960 + zone,
                (true, false) => return None,
                (false, true) => 32700 + zone,
                (false, false) => 32600 + zone,
            })
            .ok();
        }
        if name.starts_with("PROJCS") || text.trim_start().starts_with("PROJCS") {
            return None;
        }
        if sirgas {
            return Some(Crs::SIRGAS2000);
        }
        (name.contains("WGS_1984") || name.contains("WGS_84") || name.contains("WGS84")).then_some(Crs::WGS84)
    }

    /// The codes mkaeGIS knows, for pickers.
    pub fn known() -> Vec<Crs> {
        let mut out = vec![Crs::WGS84, Crs::SIRGAS2000, Crs::WEB_MERCATOR];
        out.extend((31978..=31985).map(Crs));
        out.extend((32601..=32660).map(Crs));
        out.extend((32701..=32760).map(Crs));
        out
    }
}

fn central_meridian(zone: u8) -> f64 {
    f64::from(zone) * 6.0 - 183.0
}

/// The transverse Mercator on an ellipsoid, by Krüger's n-series to fourth order.
struct TransverseMercator {
    n: f64,
    big_a: f64,
    alpha: [f64; 4],
    beta: [f64; 4],
    delta: [f64; 4],
}

impl TransverseMercator {
    fn new(e: Ellipsoid) -> Self {
        let f = 1.0 / e.inv_f;
        let n = f / (2.0 - f);
        let (n2, n3, n4) = (n * n, n * n * n, n * n * n * n);
        TransverseMercator {
            n,
            big_a: e.a / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0),
            alpha: [
                n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0,
                13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0,
                61.0 * n3 / 240.0 - 103.0 * n4 / 140.0,
                49561.0 * n4 / 161_280.0,
            ],
            beta: [
                n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0,
                n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0,
                17.0 * n3 / 480.0 - 37.0 * n4 / 840.0,
                4397.0 * n4 / 161_280.0,
            ],
            delta: [
                2.0 * n - 2.0 * n2 / 3.0 - 2.0 * n3 + 116.0 * n4 / 45.0,
                7.0 * n2 / 3.0 - 8.0 * n3 / 5.0 - 227.0 * n4 / 45.0,
                56.0 * n3 / 15.0 - 136.0 * n4 / 35.0,
                4279.0 * n4 / 630.0,
            ],
        }
    }

    /// Easting and northing (metres, no false origin) of a point `dlon` degrees from the central meridian.
    fn forward(&self, dlon: f64, lat: f64) -> (f64, f64) {
        let (phi, lam) = (lat.to_radians(), dlon.to_radians());
        let c = 2.0 * self.n.sqrt() / (1.0 + self.n);
        let s = phi.sin().clamp(-1.0 + 1e-15, 1.0 - 1e-15);
        let t = (s.atanh() - c * (c * s).atanh()).sinh();
        let xi = t.atan2(lam.cos());
        let eta = (lam.sin() / (1.0 + t * t).sqrt()).atanh();
        let (mut e, mut n) = (eta, xi);
        for (j, a) in self.alpha.iter().enumerate() {
            let k = 2.0 * (j as f64 + 1.0);
            e += a * (k * xi).cos() * (k * eta).sinh();
            n += a * (k * xi).sin() * (k * eta).cosh();
        }
        (UTM_K0 * self.big_a * e, UTM_K0 * self.big_a * n)
    }

    /// Longitude and latitude (degrees) of an easting and northing (no false origin).
    fn inverse(&self, x: f64, y: f64, lon0: f64) -> (f64, f64) {
        let (xi, eta) = (y / (UTM_K0 * self.big_a), x / (UTM_K0 * self.big_a));
        let (mut xi1, mut eta1) = (xi, eta);
        for (j, b) in self.beta.iter().enumerate() {
            let k = 2.0 * (j as f64 + 1.0);
            xi1 -= b * (k * xi).sin() * (k * eta).cosh();
            eta1 -= b * (k * xi).cos() * (k * eta).sinh();
        }
        let chi = (xi1.sin() / eta1.cosh()).clamp(-1.0, 1.0).asin();
        let mut phi = chi;
        for (j, d) in self.delta.iter().enumerate() {
            phi += d * (2.0 * (j as f64 + 1.0) * chi).sin();
        }
        let lam = eta1.sinh().atan2(xi1.cos());
        (lon0 + lam.to_degrees(), phi.to_degrees())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn web_mercator_reference_values() {
        // R·π and R·ln(tan(π/4 + 22.5°)), computed independently.
        let c = Crs::WEB_MERCATOR.from_geographic(Coord::new(180.0, 45.0)).unwrap();
        assert!(close(c.x, 20_037_508.342_789_244, 1e-6) && close(c.y, 5_621_521.486_192_066, 1e-6));
        let back = Crs::WEB_MERCATOR.to_geographic(c).unwrap();
        assert!(close(back.x, 180.0, 1e-9) && close(back.y, 45.0, 1e-9));
    }

    #[test]
    fn utm_on_the_central_meridian_is_the_scaled_meridian_arc() {
        // 0.9996 × the WGS 84 meridian arc, by numerical integration (Simpson, 200 000 steps).
        let c = Crs::from_epsg(32632).unwrap().from_geographic(Coord::new(9.0, 45.0)).unwrap();
        assert!(close(c.x, 500_000.0, 1e-6) && close(c.y, 4_982_950.400_226_577, 1e-3), "{c:?}");
        let s = Crs::from_epsg(32723).unwrap().from_geographic(Coord::new(-45.0, -23.5)).unwrap();
        assert!(close(s.x, 500_000.0, 1e-6) && close(s.y, 10_000_000.0 - 2_598_871.375_431_317_4, 1e-3), "{s:?}");
    }

    #[test]
    fn utm_round_trips_across_the_zone() {
        let utm = Crs::from_epsg(31983).unwrap();
        for lon in [-48.0, -46.6, -45.0, -43.2, -42.0] {
            for lat in [-33.0, -23.55, -5.0, -0.1] {
                let p = utm.from_geographic(Coord::new(lon, lat)).unwrap();
                let g = utm.to_geographic(p).unwrap();
                assert!(close(g.x, lon, 1e-9) && close(g.y, lat, 1e-9), "{lon} {lat} -> {g:?}");
            }
        }
        // East of the central meridian is east of 500 km.
        assert!(utm.from_geographic(Coord::new(-44.0, -23.0)).unwrap().x > 500_000.0);
        assert!(utm.from_geographic(Coord::new(120.0, 0.0)).is_err());
    }

    #[test]
    fn codes_names_and_zones() {
        assert_eq!(Crs::utm_for(-46.6, -23.5).unwrap().epsg(), 32723);
        assert_eq!(Crs::utm_for(3.0, 10.0).unwrap().epsg(), 32631);
        assert_eq!(Crs::utm_for(180.0, 10.0).unwrap().epsg(), 32601);
        assert_eq!(Crs::from_epsg(31982).unwrap().name(), "SIRGAS 2000 / UTM zone 22S");
        assert!(Crs::from_epsg(0).is_err() && Crs::from_epsg(32661).is_err());
        assert_eq!(Crs::from_epsg(102_100).unwrap(), Crs::WEB_MERCATOR);
    }

    #[test]
    fn prj_round_trips() {
        for crs in Crs::known() {
            assert_eq!(Crs::from_wkt(&crs.to_prj()), Some(crs), "{}", crs.name());
        }
        let ogc = r#"PROJCS["WGS 84 / UTM zone 23S",GEOGCS["WGS 84",AUTHORITY["EPSG","4326"]],AUTHORITY["EPSG","32723"]]"#;
        assert_eq!(Crs::from_wkt(ogc).map(Crs::epsg), Some(32723));
        assert_eq!(Crs::from_wkt(r#"PROJCS["Lambert_Conformal_Conic",GEOGCS["GCS_WGS_1984"]]"#), None);
        assert_eq!(Crs::from_wkt(""), None);
    }
}
