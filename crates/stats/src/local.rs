//! Local statistics: where the clusters, outliers and hot spots are.
//!
//! - Local Moran's Iᵢ (Anselin 1995): Iᵢ = (zᵢ / m₂) · Σⱼ wᵢⱼ zⱼ, with zᵢ = xᵢ − x̄ and m₂ = Σ zᵢ² / n,
//!   so that Σᵢ Iᵢ = S0 · I (the global Moran's I). Its expectation −wᵢ. / (n − 1) and variance
//!   under total randomisation are Anselin's (1995, eq. 13–14), for the z-score; its pseudo
//!   p-value comes from a conditional permutation: xᵢ stays, and its neighbours' values are drawn from the other
//!   n − 1 without replacement. Each feature is labelled by the quadrant of the Moran scatter
//!   plot it falls in (High-High, Low-Low, High-Low, Low-High) when significant.
//! - Getis–Ord Gi* (Getis and Ord 1992; Ord and Getis 1995): the standardised
//!   Gi* = (Σⱼ wᵢⱼ xⱼ − x̄ Wᵢ) / (S √((n S1ᵢ − Wᵢ²) / (n − 1))), with the feature itself among
//!   its neighbours, Wᵢ = Σⱼ wᵢⱼ, S1ᵢ = Σⱼ wᵢⱼ² and S² = Σ xⱼ² / n − x̄². It is a z-score under
//!   randomisation; its p-value is two-sided normal.
//!
//! Many local tests at once need a correction: Bonferroni (α / m) or the false discovery rate
//! (Benjamini and Hochberg 1995, as Caldas de Castro and Singer 2006 apply it to LISA).

use mkaegis_core::{Error, Result};

use crate::inference::{Rng, two_sided_p};

/// The most permutations a local test runs.
pub const MAX_PERMUTATIONS: u32 = 99_999;

/// How many tests at once are allowed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    None,
    Bonferroni,
    /// Benjamini–Hochberg.
    Fdr,
}

impl Correction {
    pub fn parse(text: &str) -> Option<Correction> {
        match text.to_ascii_lowercase().as_str() {
            "none" => Some(Correction::None),
            "bonferroni" => Some(Correction::Bonferroni),
            "fdr" | "bh" | "benjamini-hochberg" => Some(Correction::Fdr),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Correction::None => "none",
            Correction::Bonferroni => "bonferroni",
            Correction::Fdr => "fdr",
        }
    }
}

/// A feature's place on the Moran scatter plot, when its Iᵢ is significant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cluster {
    HighHigh,
    LowLow,
    HighLow,
    LowHigh,
    NotSignificant,
    /// No neighbours: nothing to test.
    Island,
}

impl Cluster {
    pub fn name(self) -> &'static str {
        match self {
            Cluster::HighHigh => "High-High",
            Cluster::LowLow => "Low-Low",
            Cluster::HighLow => "High-Low",
            Cluster::LowHigh => "Low-High",
            Cluster::NotSignificant => "Not significant",
            Cluster::Island => "No neighbours",
        }
    }
}

/// Local Moran's I for every feature.
#[derive(Debug, Clone, PartialEq)]
pub struct Lisa {
    /// Per feature; `None` for a feature without neighbours.
    pub i: Vec<Option<f64>>,
    pub expected: Vec<Option<f64>>,
    /// z under randomisation.
    pub z: Vec<Option<f64>>,
    /// The p-value tested: the pseudo p-value with permutations, else two-sided normal.
    pub p: Vec<Option<f64>>,
    pub cluster: Vec<Cluster>,
    pub summary: Summary,
}

/// Getis–Ord Gi* for every feature.
#[derive(Debug, Clone, PartialEq)]
pub struct HotSpots {
    pub z: Vec<f64>,
    pub p: Vec<f64>,
    /// 3, 2, 1 for a hot spot at 99, 95, 90 % confidence; −3, −2, −1 for a cold spot; 0 for none.
    pub bin: Vec<i8>,
    pub summary: Summary,
}

/// What was tested and how.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Summary {
    /// Features tested (those with neighbours).
    pub tested: usize,
    pub correction: &'static str,
    pub alpha: f64,
    /// The p-value at or below which a test is significant at `alpha` after the correction.
    pub threshold: f64,
    pub significant: usize,
    pub permutations: u32,
    pub seed: u64,
}

/// Local Moran's I of `values` for weights `rows`, tested at `alpha` after `correction`, by
/// `permutations` conditional permutations from `seed` (0: by the randomisation z instead).
pub fn local_moran(values: &[f64], rows: &[Vec<(u32, f64)>], permutations: u32, seed: u64, alpha: f64, correction: Correction) -> Result<Lisa> {
    let n = check(values, rows, permutations, alpha)?;
    let nf = n as f64;
    let mean = values.iter().sum::<f64>() / nf;
    let z: Vec<f64> = values.iter().map(|x| x - mean).collect();
    let m2 = z.iter().map(|d| d * d).sum::<f64>() / nf;
    let m4 = z.iter().map(|d| d.powi(4)).sum::<f64>() / nf;
    let b2 = m4 / (m2 * m2);
    let at = |j: u32| z.get(j as usize).copied().unwrap_or(0.0);
    let mut rng = Rng::new(seed);
    let mut others: Vec<usize> = Vec::with_capacity(n);
    let mut out =
        Lisa { i: vec![None; n], expected: vec![None; n], z: vec![None; n], p: vec![None; n], cluster: vec![Cluster::Island; n], summary: empty() };
    let mut lags = vec![0.0; n];
    for (i, row) in rows.iter().enumerate() {
        // A feature's own weight isn't a neighbour's: Iᵢ is about the others.
        let row: Vec<(u32, f64)> = row.iter().copied().filter(|(j, _)| *j as usize != i).collect();
        if row.is_empty() {
            continue;
        }
        let zi = z.get(i).copied().unwrap_or(0.0);
        let lag: f64 = row.iter().map(|(j, w)| w * at(*j)).sum();
        let ii = zi / m2 * lag;
        let wi: f64 = row.iter().map(|(_, w)| w).sum();
        let wi2: f64 = row.iter().map(|(_, w)| w * w).sum();
        let wikh = wi * wi - wi2;
        let e = -wi / (nf - 1.0);
        let var = wi2 * (nf - b2) / (nf - 1.0) + wikh * (2.0 * b2 - nf) / ((nf - 1.0) * (nf - 2.0)) - e * e;
        let zscore = (ii - e) / var.sqrt();
        let p = if permutations > 0 {
            others.clear();
            others.extend((0..n).filter(|j| *j != i));
            let k = row.len().min(others.len());
            let (mut at_least, mut at_most) = (0u32, 0u32);
            let (mut sum, mut count) = (0.0, 0u32);
            for _ in 0..permutations {
                // A partial Fisher–Yates draws k of the other values without replacement.
                for s in 0..k {
                    let r = s + rng.below(others.len() - s);
                    others.swap(s, r);
                }
                let lag_p: f64 = row.iter().zip(&others).map(|((_, w), j)| w * z.get(*j).copied().unwrap_or(0.0)).sum();
                let ip = zi / m2 * lag_p;
                at_least += u32::from(ip >= ii);
                at_most += u32::from(ip <= ii);
                sum += ip;
                count += 1;
            }
            let extreme = if ii >= sum / f64::from(count.max(1)) { at_least } else { at_most };
            (f64::from(extreme) + 1.0) / (f64::from(permutations) + 1.0)
        } else {
            two_sided_p(zscore)
        };
        out.i[i] = Some(ii);
        out.expected[i] = Some(e);
        out.z[i] = Some(zscore);
        out.p[i] = Some(p);
        lags[i] = lag;
    }
    let (threshold, tested) = threshold(out.p.iter().flatten().copied(), alpha, correction);
    let mut significant = 0;
    for (i, (cluster, lag)) in out.cluster.iter_mut().zip(&lags).enumerate() {
        let Some(p) = out.p.get(i).copied().flatten() else { continue };
        let (zi, lag) = (z.get(i).copied().unwrap_or(0.0), *lag);
        *cluster = if p > threshold {
            Cluster::NotSignificant
        } else {
            significant += 1;
            match (zi >= 0.0, lag >= 0.0) {
                (true, true) => Cluster::HighHigh,
                (false, false) => Cluster::LowLow,
                (true, false) => Cluster::HighLow,
                (false, true) => Cluster::LowHigh,
            }
        };
    }
    out.summary = Summary { tested, correction: correction.name(), alpha, threshold, significant, permutations, seed };
    Ok(out)
}

/// Getis–Ord Gi* of `values` for weights `rows`. A feature is its own neighbour: when a row has
/// no weight for itself, it takes the row's largest weight (1 for binary weights, 1/k for
/// row-standardised k nearest neighbours), so it counts like its closest neighbour. Bins are by
/// the two-sided p-value at 0.01, 0.05 and 0.10, each after `correction`.
pub fn hot_spots(values: &[f64], rows: &[Vec<(u32, f64)>], correction: Correction) -> Result<HotSpots> {
    let n = check(values, rows, 0, 0.05)?;
    let nf = n as f64;
    let mean = values.iter().sum::<f64>() / nf;
    let s = (values.iter().map(|x| x * x).sum::<f64>() / nf - mean * mean).max(0.0).sqrt();
    if s == 0.0 {
        return Err(Error::param("field", "every value is the same: there is no variation to test"));
    }
    let mut z = Vec::with_capacity(n);
    for (i, row) in rows.iter().enumerate() {
        let mut star: Vec<(u32, f64)> = row.clone();
        if !star.iter().any(|(j, _)| *j as usize == i) {
            let own = star.iter().map(|(_, w)| *w).fold(0.0, f64::max);
            star.push((u32::try_from(i).map_err(|_| Error::param("weights", "too many features"))?, if own > 0.0 { own } else { 1.0 }));
        }
        let wi: f64 = star.iter().map(|(_, w)| w).sum();
        let s1: f64 = star.iter().map(|(_, w)| w * w).sum();
        let sum: f64 = star.iter().map(|(j, w)| w * values.get(*j as usize).copied().unwrap_or(0.0)).sum();
        let den = s * ((nf * s1 - wi * wi) / (nf - 1.0)).sqrt();
        z.push(if den > 0.0 { (sum - mean * wi) / den } else { 0.0 });
    }
    let p: Vec<f64> = z.iter().map(|z| two_sided_p(*z)).collect();
    let cut = |alpha: f64| threshold(p.iter().copied(), alpha, correction).0;
    let (t99, t95, t90) = (cut(0.01), cut(0.05), cut(0.10));
    let bin: Vec<i8> = z
        .iter()
        .zip(&p)
        .map(|(z, p)| {
            let level = if *p <= t99 {
                3
            } else if *p <= t95 {
                2
            } else if *p <= t90 {
                1
            } else {
                0
            };
            if *z < 0.0 { -level } else { level }
        })
        .collect();
    let significant = p.iter().filter(|p| **p <= t95).count();
    let summary = Summary { tested: n, correction: correction.name(), alpha: 0.05, threshold: t95, significant, permutations: 0, seed: 0 };
    Ok(HotSpots { z, p, bin, summary })
}

/// The p-value at or below which a test is significant at `alpha` among the given p-values,
/// and how many there are. Bonferroni: α / m. FDR: the largest p₍ₖ₎ ≤ k α / m (0 if none).
pub fn threshold(p: impl Iterator<Item = f64>, alpha: f64, correction: Correction) -> (f64, usize) {
    let mut p: Vec<f64> = p.collect();
    let m = p.len();
    let t = match correction {
        Correction::None => alpha,
        Correction::Bonferroni => alpha / m.max(1) as f64,
        Correction::Fdr => {
            p.sort_by(f64::total_cmp);
            p.iter().enumerate().filter(|(k, pk)| **pk <= (*k as f64 + 1.0) * alpha / m as f64).map(|(_, pk)| *pk).next_back().unwrap_or(0.0)
        }
    };
    (t, m)
}

fn check(values: &[f64], rows: &[Vec<(u32, f64)>], permutations: u32, alpha: f64) -> Result<usize> {
    let n = values.len();
    if rows.len() != n {
        return Err(Error::param("weights", format!("the weights have {} rows and the field {n} values", rows.len())));
    }
    if n < 4 {
        return Err(Error::param("field", "needs at least 4 features"));
    }
    if let Some(i) = values.iter().position(|x| !x.is_finite()) {
        return Err(Error::param("field", format!("row {} is not a finite number", i + 1)));
    }
    if values.iter().all(|x| *x == values[0]) {
        return Err(Error::param("field", "every value is the same: there is no variation to test"));
    }
    if permutations > MAX_PERMUTATIONS {
        return Err(Error::param("permutations", format!("up to {MAX_PERMUTATIONS}")));
    }
    if !(alpha > 0.0 && alpha < 1.0) {
        return Err(Error::param("alpha", "a significance level between 0 and 1"));
    }
    for (i, row) in rows.iter().enumerate() {
        if let Some((j, w)) = row.iter().find(|(j, w)| *j as usize >= n || !w.is_finite()) {
            return Err(Error::param("weights", format!("row {}: neighbour {j} with weight {w} doesn't fit", i + 1)));
        }
    }
    if rows.iter().all(Vec::is_empty) {
        return Err(Error::param("weights", "no feature has a neighbour"));
    }
    Ok(n)
}

fn empty() -> Summary {
    Summary { tested: 0, correction: "none", alpha: 0.05, threshold: 0.05, significant: 0, permutations: 0, seed: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autocorrelation::{Statistic, global};

    const VALUES: [f64; 6] = [8.0, 6.0, 1.0, 7.0, 3.0, 2.0];

    mod moran {
        use super::*;

        /// Σ Iᵢ = S0 · I: with row-standardised weights S0 = n, so the mean Iᵢ is the global I.
        #[test]
        fn local_values_add_up_to_the_global_one() {
            for (w, s0) in [(rook_2x3(true), 6.0), (rook_2x3(false), 14.0)] {
                let lisa = local_moran(&VALUES, &w, 0, 0, 0.05, Correction::None).unwrap();
                let sum: f64 = lisa.i.iter().flatten().sum();
                let global = global(Statistic::MoranI, &VALUES, &w, 0, 0).unwrap().value;
                assert!((sum - s0 * global).abs() < 1e-12, "{sum} vs {}", s0 * global);
            }
        }

        /// Under total randomisation, Iᵢ's mean and variance are its mean and variance over all
        /// 720 arrangements of the values on the locations.
        #[test]
        fn moments_match_every_arrangement() {
            for w in [rook_2x3(false), rook_2x3(true)] {
                let lisa = local_moran(&VALUES, &w, 0, 0, 0.05, Correction::None).unwrap();
                let all: Vec<Lisa> = permutations(&VALUES).iter().map(|v| local_moran(v, &w, 0, 0, 0.05, Correction::None).unwrap()).collect();
                for i in 0..VALUES.len() {
                    let values: Vec<f64> = all.iter().map(|l| l.i[i].unwrap()).collect();
                    let mean = values.iter().sum::<f64>() / values.len() as f64;
                    let var = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / values.len() as f64;
                    let (e, z, ii) = (lisa.expected[i].unwrap(), lisa.z[i].unwrap(), lisa.i[i].unwrap());
                    assert!((e - mean).abs() < 1e-12, "row {i}: E {e} vs {mean}");
                    let sd = (ii - e) / z;
                    assert!((sd * sd - var).abs() < 1e-12 * var.max(1.0), "row {i}: Var {} vs {var}", sd * sd);
                }
            }
        }

        #[test]
        fn permutations_are_reproducible_and_close_to_exact() {
            let w = rook_2x3(false);
            let a = local_moran(&VALUES, &w, 9_999, 3, 0.05, Correction::None).unwrap();
            assert_eq!(a, local_moran(&VALUES, &w, 9_999, 3, 0.05, Correction::None).unwrap());
            // Feature 0's exact one-sided p over the 120 arrangements of the others.
            let others: Vec<f64> = VALUES[1..].to_vec();
            let all: Vec<f64> = permutations(&others)
                .iter()
                .map(|o| {
                    let mut v = o.clone();
                    v.insert(0, VALUES[0]);
                    local_moran(&v, &w, 0, 0, 0.05, Correction::None).unwrap().i[0].unwrap()
                })
                .collect();
            let ii = a.i[0].unwrap();
            let exact = all.iter().filter(|x| **x >= ii - 1e-12).count() as f64 / all.len() as f64;
            assert!((a.p[0].unwrap() - exact).abs() < 0.01, "{} vs {exact}", a.p[0].unwrap());
        }

        /// A planted pattern on a 10 × 10 grid: a high 4 × 4 corner (100) and a low one (0) on a
        /// middling background (50 ± 3), a low value amid the highs and a high one amid the lows.
        #[test]
        fn a_planted_pattern_gets_its_cluster_labels() {
            let values = planted();
            let w = queen_grid(10, true);
            let lisa = local_moran(&values, &w, 999, 1, 0.05, Correction::None).unwrap();
            assert_eq!(lisa.cluster[22], Cluster::HighHigh);
            assert_eq!(lisa.cluster[77], Cluster::LowLow);
            assert_eq!(lisa.cluster[11], Cluster::LowHigh);
            assert_eq!(lisa.cluster[88], Cluster::HighLow);
            assert_eq!(lisa.summary.tested, 100);
            // Corrections only ever make fewer features significant.
            let fdr = local_moran(&values, &w, 999, 1, 0.05, Correction::Fdr).unwrap();
            let bonf = local_moran(&values, &w, 999, 1, 0.05, Correction::Bonferroni).unwrap();
            assert!(bonf.summary.significant <= fdr.summary.significant && fdr.summary.significant <= lisa.summary.significant);
        }

        #[test]
        fn islands_are_not_tested() {
            let mut w = rook_2x3(false);
            w[5].clear();
            for row in &mut w {
                row.retain(|(j, _)| *j != 5);
            }
            let lisa = local_moran(&VALUES, &w, 99, 1, 0.05, Correction::None).unwrap();
            assert_eq!((lisa.i[5], lisa.p[5], lisa.cluster[5]), (None, None, Cluster::Island));
            assert_eq!(lisa.summary.tested, 5);
        }
    }

    mod getis_ord {
        use super::*;

        /// Under randomisation (all n values permuted over the locations), Σⱼ wᵢⱼ xⱼ with the
        /// feature included has mean x̄ Wᵢ and variance S² (n S1ᵢ − Wᵢ²) / (n − 1): checked over
        /// all 720 arrangements, so Gi* is that sum's z-score.
        #[test]
        fn gi_star_is_the_exact_randomisation_z() {
            let w = rook_2x3(false);
            let hs = hot_spots(&VALUES, &w, Correction::None).unwrap();
            for (i, row) in w.iter().enumerate() {
                let star: Vec<(u32, f64)> = row.iter().copied().chain([(i as u32, 1.0)]).collect();
                let sum = |v: &[f64]| star.iter().map(|(j, wt)| wt * v[*j as usize]).sum::<f64>();
                let all: Vec<f64> = permutations(&VALUES).iter().map(|v| sum(v)).collect();
                let mean = all.iter().sum::<f64>() / all.len() as f64;
                let sd = (all.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / all.len() as f64).sqrt();
                let z = (sum(&VALUES) - mean) / sd;
                assert!((hs.z[i] - z).abs() < 1e-12, "row {i}: {} vs {z}", hs.z[i]);
            }
        }

        /// By hand, feature 0 of the binary rook 2 × 3 lattice: itself and features 1 and 3, so
        /// Wᵢ = 3, S1ᵢ = 3, Σ wᵢⱼ xⱼ = 8 + 6 + 7 = 21; x̄ = 4.5, S² = 179/6 − 20.25 = 41.5/6. Then
        /// Gi* = (21 − 13.5) / (S √((6·3 − 9)/5)) = 7.5 / (√(41.5/6) · √1.8).
        #[test]
        fn hand_computed_gi_star() {
            let hs = hot_spots(&VALUES, &rook_2x3(false), Correction::None).unwrap();
            let expected = 7.5 / ((41.5f64 / 6.0).sqrt() * 1.8f64.sqrt());
            assert!((hs.z[0] - expected).abs() < 1e-12, "{} vs {expected}", hs.z[0]);
            // Row-standardising the weights doesn't change Gi*: the feature counts like a neighbour.
            let row = hot_spots(&VALUES, &rook_2x3(true), Correction::None).unwrap();
            assert!((row.z[0] - expected).abs() < 1e-12);
        }

        #[test]
        fn a_planted_hot_spot_is_binned() {
            let mut values = planted();
            values[11] = 100.0;
            values[88] = 0.0;
            let hs = hot_spots(&values, &queen_grid(10, false), Correction::Fdr).unwrap();
            assert_eq!((hs.bin[11], hs.bin[88]), (3, -3), "the hot and cold spots' centres");
            assert_eq!(hs.bin[55], 0, "the background");
            assert!(hs.bin.iter().all(|b| (-3..=3).contains(b)));
        }
    }

    #[test]
    fn corrections() {
        let p = [0.001, 0.008, 0.039, 0.041, 0.042, 0.06, 0.074, 0.205, 0.212, 0.216];
        assert_eq!(threshold(p.iter().copied(), 0.05, Correction::None), (0.05, 10));
        assert_eq!(threshold(p.iter().copied(), 0.05, Correction::Bonferroni).0, 0.005);
        // Benjamini–Hochberg: p₍₂₎ = 0.008 ≤ 2·0.05/10 = 0.01, p₍₃₎ = 0.039 > 0.015.
        assert_eq!(threshold(p.iter().copied(), 0.05, Correction::Fdr).0, 0.008);
        assert_eq!(threshold([0.5, 0.9].into_iter(), 0.05, Correction::Fdr).0, 0.0);
        assert_eq!(Correction::parse("FDR"), Some(Correction::Fdr));
    }

    #[test]
    fn bad_input_is_refused() {
        let w = rook_2x3(false);
        assert!(local_moran(&VALUES[..5], &w, 0, 0, 0.05, Correction::None).is_err());
        assert!(local_moran(&[2.0; 6], &w, 0, 0, 0.05, Correction::None).is_err());
        assert!(local_moran(&VALUES, &w, 0, 0, 1.5, Correction::None).is_err());
        assert!(local_moran(&VALUES, &w, MAX_PERMUTATIONS + 1, 0, 0.05, Correction::None).is_err());
        assert!(hot_spots(&VALUES, &vec![Vec::new(); 6], Correction::None).is_err());
        assert!(hot_spots(&[1.0, 2.0, f64::INFINITY, 4.0, 5.0, 6.0], &w, Correction::None).is_err());
    }

    /// See `a_planted_pattern_gets_its_cluster_labels`.
    fn planted() -> Vec<f64> {
        let mut values: Vec<f64> = (0..100)
            .map(|k| match (k / 10, k % 10) {
                (r, c) if r < 4 && c < 4 => 100.0,
                (r, c) if r > 5 && c > 5 => 0.0,
                _ => 47.0 + (k % 7) as f64,
            })
            .collect();
        values[11] = 0.0;
        values[88] = 100.0;
        values
    }

    /// The 2 × 3 rook lattice, rows 0 1 2 / 3 4 5.
    fn rook_2x3(standardise: bool) -> Vec<Vec<(u32, f64)>> {
        let adj: [&[u32]; 6] = [&[1, 3], &[0, 2, 4], &[1, 5], &[0, 4], &[1, 3, 5], &[2, 4]];
        adj.iter().map(|row| row.iter().map(|j| (*j, if standardise { 1.0 / row.len() as f64 } else { 1.0 })).collect()).collect()
    }

    /// Queen contiguity on a side × side grid, row by row.
    fn queen_grid(side: usize, standardise: bool) -> Vec<Vec<(u32, f64)>> {
        (0..side * side)
            .map(|k| {
                let (r, c) = ((k / side) as i64, (k % side) as i64);
                let mut row = Vec::new();
                for dr in -1..=1 {
                    for dc in -1..=1 {
                        let (rr, cc) = (r + dr, c + dc);
                        if (dr, dc) != (0, 0) && (0..side as i64).contains(&rr) && (0..side as i64).contains(&cc) {
                            row.push(((rr * side as i64 + cc) as u32, 1.0));
                        }
                    }
                }
                let n = row.len() as f64;
                if standardise {
                    for w in &mut row {
                        w.1 = 1.0 / n;
                    }
                }
                row
            })
            .collect()
    }

    /// Every ordering of `values` (Heap's algorithm, iterative).
    fn permutations(values: &[f64]) -> Vec<Vec<f64>> {
        let mut a = values.to_vec();
        let mut c = vec![0; a.len()];
        let mut out = vec![a.clone()];
        let mut i = 0;
        while i < a.len() {
            if c[i] < i {
                a.swap(if i % 2 == 0 { 0 } else { c[i] }, i);
                out.push(a.clone());
                c[i] += 1;
                i = 0;
            } else {
                c[i] = 0;
                i += 1;
            }
        }
        out
    }
}
