//! The sample map: a made-up archipelago in the open South Atlantic, generated in code from a
//! fixed seed — islands (one with a lake), towns, roads and rivers, with Portuguese names that
//! belong to no real place. No real-world data, nobody else's sample files.

use mkaegis_core::{Result, Value};
use mkaegis_crs::Crs;
use mkaegis_geom::{Coord, Geometry, GeometryType, algo};
use mkaegis_model::{FeatureClass, Field, FieldType, Labels, Map};

/// A small deterministic generator (64-bit LCG, top bits).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

/// A closed, island-like ring around `c`: a circle wobbled by a few harmonics, round on a
/// Mercator map (x stretched by 1/cos φ).
fn blob(rng: &mut Rng, c: Coord, r: f64, n: usize) -> Vec<Coord> {
    let harmonics: Vec<(f64, f64, f64)> = (2..9).map(|k| (f64::from(k), rng.range(0.02, 0.22) / f64::from(k).sqrt(), rng.range(0.0, 6.3))).collect();
    let stretch = 1.0 / c.y.to_radians().cos();
    let mut ring: Vec<Coord> = (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            let k: f64 = 1.0 + harmonics.iter().map(|(k, a, p)| a * (k * t + p).sin()).sum::<f64>();
            Coord::new(c.x + r * k * t.cos() * stretch, c.y + r * k * t.sin())
        })
        .collect();
    if let Some(first) = ring.first().copied() {
        ring.push(first);
    }
    ring
}

const TOWNS: [&str; 23] = [
    "Porto Celeste",
    "Vila das Gaivotas",
    "Santa Brisa",
    "Pedra do Farol",
    "Barra Clara",
    "Alto da Neblina",
    "Remanso",
    "Campo das Conchas",
    "Ventania",
    "São Coral",
    "Lagoa Funda",
    "Mirante",
    "Recanto Azul",
    "Boa Maré",
    "Serra Mansa",
    "Areia Branca do Sul",
    "Ponta do Vento",
    "Cais Velho",
    "Nova Enseada",
    "Duna Alta",
    "Ilhéu de Baixo",
    "Ancoradouro",
    "Rochedo",
];

/// The sample layers (WGS 84) added to `map`, which is fitted to them.
pub fn add_to(map: &mut Map) -> Result<()> {
    let mut rng = Rng(0x00C0_FFEE_5EED_2026);
    let islands_spec = [
        ("Ilha Grande do Mar", Coord::new(-25.0, -24.0), 0.9, 16),
        ("Ilha da Bruma", Coord::new(-23.35, -23.15), 0.42, 5),
        ("Ilhota do Sul", Coord::new(-24.25, -25.45), 0.28, 2),
    ];

    let mut islands = FeatureClass::new(
        "Ilhas",
        GeometryType::Polygon,
        Crs::WGS84,
        vec![Field::new("NOME", FieldType::Text), Field::new("TIPO", FieldType::Text), Field::new("RESERVA", FieldType::Short)],
    );
    let mut shapes = Vec::new();
    for (i, (name, c, r, _)) in islands_spec.iter().enumerate() {
        let mut rings = vec![blob(&mut rng, *c, *r, 220)];
        if i == 0 {
            // The big island's lake: a hole.
            rings.push(blob(&mut rng, Coord::new(c.x - 0.2, c.y + 0.22), 0.11, 60));
        }
        let g = Geometry::polygon(rings)?;
        shapes.push(g.clone());
        let kind = if i == 0 { "Ilha principal" } else { "Ilha secundária" };
        islands.push(Some(g), vec![Value::Text((*name).into()), Value::Text(kind.into()), Value::Int(i64::from(i == 2))])?;
    }

    // Towns: random points on land, kept apart.
    let mut towns = FeatureClass::new(
        "Cidades",
        GeometryType::Point,
        Crs::WGS84,
        vec![
            Field::new("NOME", FieldType::Text),
            Field::new("POP", FieldType::Integer),
            Field::new("CAPITAL", FieldType::Short),
            Field::new("ILHA", FieldType::Text),
        ],
    );
    let mut names = TOWNS.iter();
    let mut placed: Vec<(Coord, usize)> = Vec::new();
    for (i, (island, _, _, count)) in islands_spec.iter().enumerate() {
        let Some(shape) = shapes.get(i) else { continue };
        let Some(env) = shape.envelope() else { continue };
        let mut on_island = 0;
        let mut tries = 0;
        while on_island < *count && tries < 5000 {
            tries += 1;
            let p = Coord::new(rng.range(env.min_x, env.max_x), rng.range(env.min_y, env.max_y));
            let coast = algo::distance_to(
                &Geometry::Polyline(match shape {
                    Geometry::Polygon(r) => r.clone(),
                    _ => Vec::new(),
                }),
                p,
            );
            if !algo::contains_point(shape, p) || coast < 0.03 || placed.iter().any(|(q, _)| q.distance(p) < 0.17) {
                continue;
            }
            let Some(name) = names.next() else { break };
            let capital = placed.is_empty();
            let pop = if capital { 412_000 } else { (rng.range(7.6, 12.0).exp()).round() as i64 };
            towns.push(
                Some(Geometry::Point(p)),
                vec![Value::Text((*name).into()), Value::Int(pop), Value::Int(i64::from(capital)), Value::Text((*island).into())],
            )?;
            placed.push((p, i));
            on_island += 1;
        }
    }

    // Roads: a minimum spanning tree of each island's towns, gently bent.
    let mut roads = FeatureClass::new(
        "Estradas",
        GeometryType::Polyline,
        Crs::WGS84,
        vec![Field::new("TIPO", FieldType::Text), Field::new("PAVIMENTO", FieldType::Text)],
    );
    for island in 0..islands_spec.len() {
        let ids: Vec<usize> = placed.iter().enumerate().filter(|(_, (_, i))| *i == island).map(|(n, _)| n).collect();
        let mut connected = vec![*ids.first().unwrap_or(&0)];
        while connected.len() < ids.len() {
            let best =
                ids.iter().filter(|b| !connected.contains(b)).flat_map(|b| connected.iter().map(move |a| (*a, *b))).min_by(|(a1, b1), (a2, b2)| {
                    let d = |a: &usize, b: &usize| placed.get(*a).zip(placed.get(*b)).map_or(f64::MAX, |(p, q)| p.0.distance(q.0));
                    d(a1, b1).total_cmp(&d(a2, b2))
                });
            let Some((a, b)) = best else { break };
            connected.push(b);
            let (Some((p, _)), Some((q, _))) = (placed.get(a), placed.get(b)) else { continue };
            let bend = rng.range(-0.12, 0.12);
            let mid = Coord::new((p.x + q.x) / 2.0 - (q.y - p.y) * bend, (p.y + q.y) / 2.0 + (q.x - p.x) * bend);
            let path: Vec<Coord> = (0..=12)
                .map(|k| {
                    let t = f64::from(k) / 12.0;
                    let u = 1.0 - t;
                    Coord::new(u * u * p.x + 2.0 * u * t * mid.x + t * t * q.x, u * u * p.y + 2.0 * u * t * mid.y + t * t * q.y)
                })
                .collect();
            let main = a == 0 || b == 0;
            roads.push(
                Some(Geometry::Polyline(vec![path])),
                vec![Value::Text(if main { "Rodovia" } else { "Estrada" }.into()), Value::Text(if main { "Asfalto" } else { "Terra" }.into())],
            )?;
        }
    }

    // Rivers: from the big island's interior down to the sea, meandering.
    let mut rivers = FeatureClass::new(
        "Rios",
        GeometryType::Polyline,
        Crs::WGS84,
        vec![Field::new("NOME", FieldType::Text), Field::new("EXTENSAO_KM", FieldType::Double)],
    );
    if let (Some(Geometry::Polygon(rings)), Some((_, c, _, _))) = (shapes.first(), islands_spec.first()) {
        let coast = rings.first().cloned().unwrap_or_default();
        for (k, name) in ["Rio das Pedras", "Rio Manso", "Rio do Farol", "Rio Escuro"].iter().enumerate() {
            let angle = std::f64::consts::TAU * (k as f64 + 0.3) / 4.0;
            let source = Coord::new(c.x + 0.18 * angle.cos(), c.y + 0.14 * angle.sin());
            let Some(mouth) = coast.get(coast.len() * (2 * k + 1) / 8).copied() else { continue };
            let path: Vec<Coord> = (0..=30)
                .map(|s| {
                    let t = f64::from(s) / 30.0;
                    let wiggle = 0.03 * (t * 14.0 + k as f64).sin() * (1.0 - t);
                    let (dx, dy) = (mouth.x - source.x, mouth.y - source.y);
                    Coord::new(source.x + dx * t - dy * wiggle, source.y + dy * t + dx * wiggle)
                })
                .collect();
            rivers.push(Some(Geometry::Polyline(vec![path])), vec![Value::Text((*name).into())])?;
        }
        mkaegis_analysis::calculate_geometry(&mut rivers, "EXTENSAO_KM", mkaegis_analysis::Property::LengthKm)?;
    }

    map.add_layer(islands);
    map.add_layer(rivers);
    map.add_layer(roads);
    let towns_id = map.add_layer(towns);
    if let Ok(layer) = map.layer_mut(&towns_id.to_string()) {
        layer.labels = Some(Labels { field: "NOME".into(), color: mkaegis_core::Color::rgb(0x20, 0x20, 0x20), size: 9.0 });
    }
    style(map);
    map.extent = map.full_extent().map(|e| e.scale(1.15));
    Ok(())
}

/// The sample's symbology: sand islands, blue rivers, grey roads, dark towns.
fn style(map: &mut Map) {
    use mkaegis_core::Color;
    use mkaegis_model::{Renderer, Symbol};
    for layer in &mut map.layers {
        let s = layer.renderer.base();
        layer.renderer = Renderer::Single(match layer.name.as_str() {
            "Ilhas" => Symbol { fill: Color::rgb(0xee, 0xe3, 0xc4), outline: Color::rgb(0x8c, 0x7b, 0x5a), width: 0.8, ..s },
            "Rios" => Symbol { outline: Color::rgb(0x4a, 0x8e, 0xc9), width: 1.4, ..s },
            "Estradas" => Symbol { outline: Color::rgb(0x9a, 0x6b, 0x4f), width: 1.2, ..s },
            _ => Symbol { fill: Color::rgb(0x2d, 0x4a, 0x6b), outline: Color::WHITE, width: 0.8, size: 7.0, ..s },
        });
    }
    map.background = Color::rgb(0xd5, 0xe8, 0xf2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_is_deterministic_and_complete() {
        let (mut a, mut b) = (Map::default(), Map::default());
        add_to(&mut a).unwrap();
        add_to(&mut b).unwrap();
        assert_eq!(a, b);
        let names: Vec<&str> = a.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Cidades", "Estradas", "Rios", "Ilhas"]);
        let count = |n: &str| a.layer(n).unwrap().data.features.len();
        assert_eq!((count("Cidades"), count("Estradas"), count("Rios"), count("Ilhas")), (23, 20, 4, 3));
        // Every town is on land.
        let islands = a.layer("Ilhas").unwrap();
        for f in &a.layer("Cidades").unwrap().data.features {
            let Some(Geometry::Point(p)) = f.geometry else { panic!("not a point") };
            assert!(islands.data.features.iter().any(|i| algo::contains_point(i.geometry.as_ref().unwrap(), p)));
        }
    }
}
