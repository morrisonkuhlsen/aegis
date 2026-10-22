//! Global spatial autocorrelation: is a variable clustered, dispersed or random in space?
//!
//! - Moran's I (Moran 1950): I = (n / S0) · Σᵢⱼ wᵢⱼ zᵢ zⱼ / Σᵢ zᵢ², with zᵢ = xᵢ − x̄.
//! - Geary's C (Geary 1954): C = (n − 1) · Σᵢⱼ wᵢⱼ (xᵢ − xⱼ)² / (2 S0 · Σᵢ zᵢ²).
//! - Getis–Ord General G (Getis and Ord 1992): G = Σᵢ≠ⱼ wᵢⱼ xᵢ xⱼ / Σᵢ≠ⱼ xᵢ xⱼ, for x ≥ 0.
//!
//! Inference by the moments under normality and under randomisation (Cliff and Ord 1981, ch. 1–2;
//! Getis and Ord 1992 for G, under randomisation only), each giving a z-score and a two-sided
//! p-value, and by a permutation test: the values are shuffled over the locations `permutations`
//! times from a stated seed, and the pseudo p-value is (M + 1) / (R + 1), where M of R
//! permutations are at least as extreme as the observed value in its direction (Anselin 1995).
//!
//! With S0 = Σᵢⱼ wᵢⱼ, S1 = ½ Σᵢⱼ (wᵢⱼ + wⱼᵢ)², S2 = Σᵢ (wᵢ. + w.ᵢ)² and the kurtosis
//! b2 = n Σ zᵢ⁴ / (Σ zᵢ²)².

use std::collections::HashMap;

use mkaegis_core::{Error, Result};
use serde::Serialize;

use crate::inference::{Rng, two_sided_p};

/// The most permutations a test runs.
pub const MAX_PERMUTATIONS: u32 = 99_999;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Statistic {
    MoranI,
    GearyC,
    GeneralG,
}

/// A global statistic, its expectation under no spatial autocorrelation and its inference.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub statistic: &'static str,
    pub value: f64,
    pub expected: f64,
    pub n: usize,
    /// Under normality (none for General G).
    pub normality: Option<Moments>,
    pub randomization: Moments,
    pub permutation: Option<Permutation>,
    /// Rows without neighbours: they add nothing to the statistic but count in n.
    pub islands: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Moments {
    pub variance: f64,
    pub z: f64,
    /// Two-sided.
    pub p: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Permutation {
    pub permutations: u32,
    pub seed: u64,
    /// The permuted values' mean and standard deviation, and the observed value's z among them.
    pub mean: f64,
    pub sd: f64,
    pub z: f64,
    /// (M + 1) / (R + 1), one-sided in the direction of the observed value.
    pub p: f64,
}

impl Statistic {
    pub fn name(self) -> &'static str {
        match self {
            Statistic::MoranI => "Moran's I",
            Statistic::GearyC => "Geary's C",
            Statistic::GeneralG => "General G",
        }
    }
}

/// The global statistic of `values` (one per row, no missing values) for weights `rows`
/// (each row's neighbours and weights), with `permutations` (0 for none) from `seed`.
pub fn global(stat: Statistic, values: &[f64], rows: &[Vec<(u32, f64)>], permutations: u32, seed: u64) -> Result<Report> {
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
    if permutations > MAX_PERMUTATIONS {
        return Err(Error::param("permutations", format!("up to {MAX_PERMUTATIONS}")));
    }
    let links: Vec<(usize, usize, f64)> = rows
        .iter()
        .enumerate()
        .flat_map(|(i, row)| row.iter().map(move |(j, w)| (i, *j as usize, *w)))
        // General G sums over i ≠ j only.
        .filter(|(i, j, _)| stat != Statistic::GeneralG || i != j)
        .collect();
    if let Some((i, j, w)) = links.iter().find(|(_, j, w)| *j >= n || !w.is_finite()) {
        return Err(Error::param("weights", format!("row {}: neighbour {j} with weight {w} doesn't fit", i + 1)));
    }
    let s = sums(&links, n);
    if s.s0 == 0.0 {
        return Err(Error::param("weights", "no feature has a neighbour"));
    }
    let islands = rows.iter().filter(|r| r.is_empty()).count();
    let nf = n as f64;
    let mean = values.iter().sum::<f64>() / nf;
    let z: Vec<f64> = values.iter().map(|x| x - mean).collect();
    let m2: f64 = z.iter().map(|d| d * d).sum();
    if m2 == 0.0 {
        return Err(Error::param("field", "every value is the same: there is no variation to test"));
    }
    let b2 = nf * z.iter().map(|d| d.powi(4)).sum::<f64>() / (m2 * m2);
    let (s0, s1, s2) = (s.s0, s.s1, s.s2);

    let (value, expected, normality, randomization_variance) = match stat {
        Statistic::MoranI => {
            let i = moran(&links, &z, s0, m2, nf);
            let e = -1.0 / (nf - 1.0);
            let e2_norm = (nf * nf * s1 - nf * s2 + 3.0 * s0 * s0) / (s0 * s0 * (nf * nf - 1.0));
            let e2_rand = (nf * ((nf * nf - 3.0 * nf + 3.0) * s1 - nf * s2 + 3.0 * s0 * s0)
                - b2 * ((nf * nf - nf) * s1 - 2.0 * nf * s2 + 6.0 * s0 * s0))
                / ((nf - 1.0) * (nf - 2.0) * (nf - 3.0) * s0 * s0);
            (i, e, Some(e2_norm - e * e), e2_rand - e * e)
        }
        Statistic::GearyC => {
            let c = geary(&links, values, s0, m2, nf);
            let var_norm = ((2.0 * s1 + s2) * (nf - 1.0) - 4.0 * s0 * s0) / (2.0 * (nf + 1.0) * s0 * s0);
            let var_rand = ((nf - 1.0) * s1 * (nf * nf - 3.0 * nf + 3.0 - (nf - 1.0) * b2)
                - 0.25 * (nf - 1.0) * s2 * (nf * nf + 3.0 * nf - 6.0 - (nf * nf - nf + 2.0) * b2)
                + s0 * s0 * (nf * nf - 3.0 - (nf - 1.0).powi(2) * b2))
                / (nf * (nf - 2.0) * (nf - 3.0) * s0 * s0);
            (c, 1.0, Some(var_norm), var_rand)
        }
        Statistic::GeneralG => {
            if values.iter().any(|x| *x < 0.0) {
                return Err(Error::param("field", "General G needs values of 0 or more"));
            }
            if links.iter().any(|(_, _, w)| *w < 0.0) {
                return Err(Error::param("weights", "General G needs weights of 0 or more"));
            }
            let m = |k: i32| values.iter().map(|x| x.powi(k)).sum::<f64>();
            let (m1, m2x, m3, m4) = (m(1), m(2), m(3), m(4));
            let pairs = m1 * m1 - m2x;
            if pairs <= 0.0 {
                return Err(Error::param("field", "General G needs at least two values above 0"));
            }
            let g = cross(&links, values) / pairs;
            let e = s0 / (nf * (nf - 1.0));
            let b0 = (nf * nf - 3.0 * nf + 3.0) * s1 - nf * s2 + 3.0 * s0 * s0;
            let b1 = -((nf * nf - nf) * s1 - 2.0 * nf * s2 + 6.0 * s0 * s0);
            let b2g = -(2.0 * nf * s1 - (nf + 3.0) * s2 + 6.0 * s0 * s0);
            let b3 = 4.0 * (nf - 1.0) * s1 - 2.0 * (nf + 1.0) * s2 + 8.0 * s0 * s0;
            let b4 = s1 - s2 + s0 * s0;
            let n4 = nf * (nf - 1.0) * (nf - 2.0) * (nf - 3.0);
            let e2 = (b0 * m2x * m2x + b1 * m4 + b2g * m1 * m1 * m2x + b3 * m1 * m3 + b4 * m1.powi(4)) / (pairs * pairs * n4);
            (g, e, None, e2 - e * e)
        }
    };
    let moments = |variance: f64| {
        let z = (value - expected) / variance.sqrt();
        Moments { variance, z, p: two_sided_p(z) }
    };
    let permutation = (permutations > 0).then(|| {
        let mut rng = Rng::new(seed);
        let mut shuffled = values.to_vec();
        // The denominators don't change when the values move: only the cross-products do.
        let pairs = values.iter().sum::<f64>().powi(2) - values.iter().map(|x| x * x).sum::<f64>();
        let mut draws = Vec::with_capacity(permutations as usize);
        for _ in 0..permutations {
            rng.shuffle(&mut shuffled);
            draws.push(match stat {
                Statistic::MoranI => {
                    let zs: Vec<f64> = shuffled.iter().map(|x| x - mean).collect();
                    moran(&links, &zs, s0, m2, nf)
                }
                Statistic::GearyC => geary(&links, &shuffled, s0, m2, nf),
                Statistic::GeneralG => cross(&links, &shuffled) / pairs,
            });
        }
        let r = f64::from(permutations);
        let mean_p = draws.iter().sum::<f64>() / r;
        let sd = (draws.iter().map(|d| (d - mean_p).powi(2)).sum::<f64>() / (r - 1.0).max(1.0)).sqrt();
        let upper = value >= mean_p;
        let extreme = draws.iter().filter(|d| if upper { **d >= value } else { **d <= value }).count();
        Permutation { permutations, seed, mean: mean_p, sd, z: (value - mean_p) / sd, p: (extreme as f64 + 1.0) / (r + 1.0) }
    });
    Ok(Report {
        statistic: stat.name(),
        value,
        expected,
        n,
        normality: normality.map(moments),
        randomization: moments(randomization_variance),
        permutation,
        islands,
    })
}

struct Sums {
    s0: f64,
    s1: f64,
    s2: f64,
}

/// S0, S1 and S2 of a weights matrix given as links (i, j, wᵢⱼ).
fn sums(links: &[(usize, usize, f64)], n: usize) -> Sums {
    let mut w: HashMap<(usize, usize), f64> = HashMap::with_capacity(links.len());
    for (i, j, x) in links {
        *w.entry((*i, *j)).or_insert(0.0) += x;
    }
    let s0 = w.values().sum();
    // ½ Σᵢⱼ (wᵢⱼ + wⱼᵢ)² over every ordered pair with a weight either way: a link with no link
    // back counts for both (i, j) and (j, i).
    let mut s1 = 0.0;
    for (&(i, j), x) in &w {
        let back = w.get(&(j, i)).copied();
        s1 += match back {
            _ if i == j => 4.0 * x * x,
            Some(b) => (x + b).powi(2),
            None => 2.0 * x * x,
        };
    }
    let s1 = s1 / 2.0;
    let (mut out, mut inc) = (vec![0.0; n], vec![0.0; n]);
    for (&(i, j), x) in &w {
        if let Some(o) = out.get_mut(i) {
            *o += x;
        }
        if let Some(o) = inc.get_mut(j) {
            *o += x;
        }
    }
    let s2 = out.iter().zip(&inc).map(|(a, b)| (a + b).powi(2)).sum();
    Sums { s0, s1, s2 }
}

fn moran(links: &[(usize, usize, f64)], z: &[f64], s0: f64, m2: f64, n: f64) -> f64 {
    let cross: f64 = links.iter().map(|(i, j, w)| w * z.get(*i).copied().unwrap_or(0.0) * z.get(*j).copied().unwrap_or(0.0)).sum();
    n / s0 * cross / m2
}

fn geary(links: &[(usize, usize, f64)], x: &[f64], s0: f64, m2: f64, n: f64) -> f64 {
    let sq: f64 = links.iter().map(|(i, j, w)| w * (x.get(*i).copied().unwrap_or(0.0) - x.get(*j).copied().unwrap_or(0.0)).powi(2)).sum();
    (n - 1.0) * sq / (2.0 * s0 * m2)
}

fn cross(links: &[(usize, usize, f64)], x: &[f64]) -> f64 {
    links.iter().map(|(i, j, w)| w * x.get(*i).copied().unwrap_or(0.0) * x.get(*j).copied().unwrap_or(0.0)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six values on a 2 × 3 lattice: few enough that every one of the 720 arrangements can be
    /// enumerated, which gives the randomisation moments exactly.
    const VALUES: [f64; 6] = [8.0, 6.0, 1.0, 7.0, 3.0, 2.0];

    mod moments {
        use super::*;

        /// Under randomisation, a statistic's mean and variance are its mean and variance over
        /// every arrangement of the values on the locations: computed here by brute force.
        #[test]
        fn randomisation_moments_match_every_permutation() {
            for (name, w) in [("rook binary", rook_2x3(false)), ("rook row-standardised", rook_2x3(true)), ("asymmetric", asymmetric())] {
                for stat in [Statistic::MoranI, Statistic::GearyC, Statistic::GeneralG] {
                    let r = global(stat, &VALUES, &w, 0, 0).unwrap();
                    let all: Vec<f64> = permutations(&VALUES).iter().map(|v| global(stat, v, &w, 0, 0).unwrap().value).collect();
                    let mean = all.iter().sum::<f64>() / all.len() as f64;
                    let var = all.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / all.len() as f64;
                    assert!((r.expected - mean).abs() < 1e-12, "{name} {stat:?}: E {} vs {mean}", r.expected);
                    assert!(
                        (r.randomization.variance - var).abs() < 1e-12 * var.max(1.0),
                        "{name} {stat:?}: Var {} vs {var}",
                        r.randomization.variance
                    );
                }
            }
        }

        /// Under normality, by hand for the binary rook 2 × 3 lattice: S0 = 14, S1 = 2·S0 = 28,
        /// S2 = Σ(2dᵢ)² = 4·(4 + 9 + 4 + 4 + 9 + 4) = 136, n = 6. Then
        /// Var(I) = (36·28 − 6·136 + 3·196) / (196·35) − 1/25 = 780/6860 − 1/25 and
        /// Var(C) = ((2·28 + 136)·5 − 4·196) / (2·7·196) = 176/2744. A simulation of 2 × 10⁶
        /// standard normal samples (outside the repository) gave 0.07376 and 0.06417.
        #[test]
        fn normality_variances_by_hand() {
            let w = rook_2x3(false);
            let moran = global(Statistic::MoranI, &VALUES, &w, 0, 0).unwrap();
            let geary = global(Statistic::GearyC, &VALUES, &w, 0, 0).unwrap();
            let (vi, vc) = (moran.normality.unwrap().variance, geary.normality.unwrap().variance);
            assert!((vi - (780.0 / 6860.0 - 1.0 / 25.0)).abs() < 1e-15, "{vi}");
            assert!((vc - 176.0 / 2744.0).abs() < 1e-15, "{vc}");
            assert!((vi - 0.073_76).abs() < 2e-4 && (vc - 0.064_17).abs() < 2e-4);
        }
    }

    /// Hand-computed: VALUES on the rook 2 × 3 lattice with binary weights. Mean 4.5,
    /// z = (3.5, 1.5, −3.5, 2.5, −1.5, −2.5), Σz² = 41.5. The 7 edges (14 directed links):
    /// Σwᵢⱼzᵢzⱼ = 2·(5.25 − 5.25 − 3.75 + 3.75 + 8.75 − 2.25 + 8.75) = 30.5, so
    /// I = 6/14 · 30.5/41.5 = 0.314_974…; Σwᵢⱼ(xᵢ−xⱼ)² = 2·(4 + 25 + 16 + 1 + 1 + 9 + 1) = 114,
    /// so C = 5 · 114 / (2 · 14 · 41.5) = 0.490_533…
    #[test]
    fn hand_computed_values() {
        let w = rook_2x3(false);
        let i = global(Statistic::MoranI, &VALUES, &w, 0, 0).unwrap();
        assert!((i.value - 6.0 / 14.0 * 30.5 / 41.5).abs() < 1e-15, "{}", i.value);
        assert_eq!(i.expected, -0.2);
        let c = global(Statistic::GearyC, &VALUES, &w, 0, 0).unwrap();
        assert!((c.value - 5.0 * 114.0 / (2.0 * 14.0 * 41.5)).abs() < 1e-15, "{}", c.value);
        // Positive autocorrelation: I above its expectation, C below 1.
        assert!(i.randomization.z > 0.0 && c.randomization.z < 0.0);
    }

    #[test]
    fn permutations_are_reproducible_and_agree_with_the_moments() {
        let w = rook_2x3(false);
        let a = global(Statistic::MoranI, &VALUES, &w, 9_999, 42).unwrap();
        let b = global(Statistic::MoranI, &VALUES, &w, 9_999, 42).unwrap();
        assert_eq!(a, b);
        let p = a.permutation.unwrap();
        assert_eq!((p.permutations, p.seed), (9_999, 42));
        // The permuted values' mean and variance approach the randomisation moments.
        assert!((p.mean - a.expected).abs() < 0.01, "{}", p.mean);
        // The variance of 9 999 draws is within about 1.4 % (one standard error) of the true one.
        assert!(
            (p.sd.powi(2) - a.randomization.variance).abs() < 0.05 * a.randomization.variance,
            "{} vs {}",
            p.sd.powi(2),
            a.randomization.variance
        );
        assert!(p.p > 0.0 && p.p <= 1.0);
        // Exact one-sided p over the 720 arrangements, to compare with.
        let all: Vec<f64> = permutations(&VALUES).iter().map(|v| global(Statistic::MoranI, v, &w, 0, 0).unwrap().value).collect();
        let exact = all.iter().filter(|x| **x >= a.value - 1e-12).count() as f64 / all.len() as f64;
        assert!((p.p - exact).abs() < 0.01, "{} vs {exact}", p.p);
        assert_ne!(
            global(Statistic::MoranI, &VALUES, &w, 999, 43).unwrap().permutation,
            global(Statistic::MoranI, &VALUES, &w, 999, 42).unwrap().permutation
        );
    }

    #[test]
    fn bad_input_is_refused() {
        let w = rook_2x3(false);
        assert!(global(Statistic::MoranI, &VALUES[..5], &w, 0, 0).is_err());
        assert!(global(Statistic::MoranI, &[1.0; 6], &w, 0, 0).is_err());
        assert!(global(Statistic::MoranI, &[1.0, 2.0, f64::NAN, 4.0, 5.0, 6.0], &w, 0, 0).is_err());
        assert!(global(Statistic::MoranI, &VALUES, &vec![Vec::new(); 6], 0, 0).is_err());
        assert!(global(Statistic::MoranI, &VALUES, &w, MAX_PERMUTATIONS + 1, 0).is_err());
        assert!(global(Statistic::GeneralG, &[-1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &w, 0, 0).is_err());
        let mut far = w.clone();
        far[0].push((99, 1.0));
        assert!(global(Statistic::MoranI, &VALUES, &far, 0, 0).is_err());
        assert!(global(Statistic::MoranI, &VALUES[..3], &w[..3], 0, 0).is_err());
    }

    /// The 2 × 3 rook lattice, rows 0 1 2 / 3 4 5.
    fn rook_2x3(standardise: bool) -> Vec<Vec<(u32, f64)>> {
        let adj: [&[u32]; 6] = [&[1, 3], &[0, 2, 4], &[1, 5], &[0, 4], &[1, 3, 5], &[2, 4]];
        adj.iter()
            .map(|row| {
                let w = if standardise { 1.0 / row.len() as f64 } else { 1.0 };
                row.iter().map(|j| (*j, w)).collect()
            })
            .collect()
    }

    /// Weights that aren't symmetric, so S1 and S2 differ from the symmetric case.
    fn asymmetric() -> Vec<Vec<(u32, f64)>> {
        vec![vec![(1, 2.0)], vec![(2, 1.0), (0, 0.5)], vec![(3, 1.0)], vec![(4, 3.0), (0, 1.0)], vec![(5, 1.0)], vec![(0, 0.25), (2, 1.5)]]
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
