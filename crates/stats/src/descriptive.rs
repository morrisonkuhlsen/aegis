//! Descriptive statistics (#28): what a statistical package's Descriptives and Explore report for
//! a variable, with frequency weights and every quantile definition of Hyndman & Fan (1996).
//!
//! Definitions, as MK Sigma (and SPSS) use them:
//! - Weights are **frequency weights**: a case of weight *w* counts as *w* cases, and N = Σw. A
//!   case whose weight is missing, zero or negative is left out.
//! - Variance and standard deviation are the **sample** ones, over N − 1.
//! - Skewness is the adjusted Fisher–Pearson G1 = N·M3 / ((N−1)(N−2)s³), kurtosis the excess
//!   G2 = (N(N+1)·M4 − 3·M2²(N−1)) / ((N−1)(N−2)(N−3)s⁴), with Mₖ = Σw(x − x̄)ᵏ; their standard
//!   errors are √(6N(N−1) / ((N−2)(N+1)(N+3))) and √(4(N²−1)·SE(G1)² / ((N−3)(N+5))).
//! - The coefficient of variation is s / |x̄|, a ratio.
//! - Quantiles are Hyndman & Fan's type 6 by default, MK Sigma's and SPSS's HAVERAGE (the weighted
//!   average at (N + 1)p), so the sibling apps agree; type 7 is R's and NumPy's. With weights, the sorted values are ranked by cumulative weight, so
//!   whole weights give exactly what repeating each case would.

use mkaegis_core::{Error, Result};
use serde::Serialize;

/// The quantile definition used when none is asked for.
pub const DEFAULT_QUANTILE_TYPE: u8 = 6;

/// A variable's descriptive statistics. A statistic that needs more cases than there are is
/// `None` (a standard deviation needs N > 1, skewness N > 2, kurtosis N > 3).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Descriptives {
    /// N: the weighted count of valid cases.
    pub n: f64,
    /// Valid cases, unweighted.
    pub cases: usize,
    /// Cases left out: a missing value, or a missing, zero or negative weight.
    pub missing: usize,
    pub sum: f64,
    pub mean: Option<f64>,
    pub variance: Option<f64>,
    pub sd: Option<f64>,
    /// Standard error of the mean, s / √N.
    pub se_mean: Option<f64>,
    pub min: Option<f64>,
    pub q1: Option<f64>,
    pub median: Option<f64>,
    pub q3: Option<f64>,
    pub max: Option<f64>,
    pub range: Option<f64>,
    pub iqr: Option<f64>,
    pub skewness: Option<f64>,
    pub se_skewness: Option<f64>,
    pub kurtosis: Option<f64>,
    pub se_kurtosis: Option<f64>,
    pub cv: Option<f64>,
    /// The percentiles asked for.
    pub percentiles: Vec<Percentile>,
}

/// A percentile: `p` percent of the weight lies at or below `value`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Percentile {
    pub p: f64,
    pub value: Option<f64>,
}

/// The descriptives of `cases`, (value, weight) pairs where a `None` value is missing, with the
/// `percentiles` asked for (0–100) by quantile `kind` (Hyndman & Fan's 1–9).
pub fn describe(cases: impl IntoIterator<Item = (Option<f64>, f64)>, percentiles: &[f64], kind: u8) -> Result<Descriptives> {
    if !(1..=9).contains(&kind) {
        return Err(Error::param("quantileType", "Hyndman–Fan type 1 to 9"));
    }
    if let Some(p) = percentiles.iter().find(|p| !(p.is_finite() && (0.0..=100.0).contains(*p))) {
        return Err(Error::param("percentiles", format!("{p} is not between 0 and 100")));
    }
    let mut valid: Vec<(f64, f64)> = Vec::new();
    let mut missing = 0;
    for (x, w) in cases {
        match x {
            Some(x) if x.is_finite() && w.is_finite() && w > 0.0 => valid.push((x, w)),
            _ => missing += 1,
        }
    }
    valid.sort_by(|a, b| a.0.total_cmp(&b.0));
    let n: f64 = valid.iter().map(|(_, w)| w).sum();
    // From +0: an empty f64 sum is -0, which would print as `-0`.
    let sum = valid.iter().fold(0.0, |a, (x, w)| a + w * x);
    let mean = (n > 0.0).then(|| sum / n);
    // Central moments from the mean, in a second pass: stable for large offsets.
    let moment = |k: i32| mean.map(|m| valid.iter().map(|(x, w)| w * (x - m).powi(k)).sum::<f64>());
    let (m2, m3, m4) = (moment(2), moment(3), moment(4));
    let variance = m2.filter(|_| n > 1.0).map(|m2| m2 / (n - 1.0));
    let sd = variance.map(f64::sqrt);
    let se_skewness = (n > 2.0).then(|| (6.0 * n * (n - 1.0) / ((n - 2.0) * (n + 1.0) * (n + 3.0))).sqrt());
    let skewness = match (m3, sd) {
        (Some(m3), Some(s)) if n > 2.0 && s > 0.0 => Some(n * m3 / ((n - 1.0) * (n - 2.0) * s.powi(3))),
        _ => None,
    };
    let kurtosis = match (m2, m4, sd) {
        (Some(m2), Some(m4), Some(s)) if n > 3.0 && s > 0.0 => {
            Some((n * (n + 1.0) * m4 - 3.0 * m2 * m2 * (n - 1.0)) / ((n - 1.0) * (n - 2.0) * (n - 3.0) * s.powi(4)))
        }
        _ => None,
    };
    let se_kurtosis = se_skewness.filter(|_| n > 3.0).map(|ses| (4.0 * (n * n - 1.0) * ses * ses / ((n - 3.0) * (n + 5.0))).sqrt());
    let q = |p: f64| quantile(&valid, n, p, kind);
    let (min, max) = (valid.first().map(|v| v.0), valid.last().map(|v| v.0));
    let (q1, q3) = (q(0.25), q(0.75));
    Ok(Descriptives {
        n,
        cases: valid.len(),
        missing,
        sum,
        mean,
        variance,
        sd,
        se_mean: sd.map(|s| s / n.sqrt()),
        min,
        q1,
        median: q(0.5),
        q3,
        max,
        range: min.zip(max).map(|(a, b)| b - a),
        iqr: q1.zip(q3).map(|(a, b)| b - a),
        skewness,
        se_skewness: se_skewness.filter(|_| skewness.is_some()),
        kurtosis,
        se_kurtosis: se_kurtosis.filter(|_| kurtosis.is_some()),
        cv: sd.zip(mean).filter(|(_, m)| *m != 0.0).map(|(s, m)| s / m.abs()),
        percentiles: percentiles.iter().map(|p| Percentile { p: *p, value: q(p / 100.0) }).collect(),
    })
}

/// The `p` quantile (0–1) of `sorted` (value, weight) pairs whose weights sum to `total`, by
/// Hyndman & Fan's definition `kind` (1–9), with N = `total`.
pub fn quantile(sorted: &[(f64, f64)], total: f64, p: f64, kind: u8) -> Option<f64> {
    if sorted.is_empty() || total <= 0.0 || !(0.0..=1.0).contains(&p) {
        return None;
    }
    let n = total;
    // The value at rank k (1-based) on the cumulative weight scale, clamped to the ends.
    let at = |k: f64| -> f64 {
        let k = k.clamp(1.0, n);
        let mut cum = 0.0;
        for (x, w) in sorted {
            cum += w;
            if cum >= k - 1e-9 {
                return *x;
            }
        }
        sorted.last().map_or(0.0, |v| v.0)
    };
    let np = n * p;
    Some(match kind {
        // Discontinuous: the inverse of the empirical distribution function.
        1 => {
            let j = np.floor();
            if np - j > 0.0 { at(j + 1.0) } else { at(j) }
        }
        // The same, averaging at discontinuities.
        2 => {
            let j = np.floor();
            if np - j > 0.0 { at(j + 1.0) } else { (at(j) + at(j + 1.0)) / 2.0 }
        }
        // The nearest even order statistic.
        3 => {
            let h = np - 0.5;
            let j = h.floor();
            if h - j == 0.0 && (j as i64) % 2 == 0 { at(j) } else { at(j + 1.0) }
        }
        // Continuous: linear interpolation at h = Np + m between order statistics ⌊h⌋ and ⌊h⌋+1.
        _ => {
            let m = match kind {
                4 => 0.0,
                5 => 0.5,
                6 => p,
                7 => 1.0 - p,
                8 => (p + 1.0) / 3.0,
                _ => p / 4.0 + 3.0 / 8.0,
            };
            let h = np + m;
            let j = h.floor();
            let (lo, hi) = (at(j), at(j + 1.0));
            if h < 1.0 {
                at(1.0)
            } else if h >= n {
                at(n)
            } else {
                lo + (h - j) * (hi - lo)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() <= TOL * b.abs().max(1.0))
    }

    fn plain(xs: &[f64]) -> Vec<(Option<f64>, f64)> {
        xs.iter().map(|x| (Some(*x), 1.0)).collect()
    }

    /// Reference values from NumPy 2 and SciPy (`skew`/`kurtosis` with `bias=False`), computed
    /// outside the repository; G1/G2's standard errors match SPSS's (0.687, 1.334 for N = 10).
    #[test]
    fn one_to_ten_matches_independent_values() {
        let xs: Vec<f64> = (1..=10).map(f64::from).collect();
        let d = describe(plain(&xs), &[5.0, 95.0], 7).unwrap();
        assert_eq!((d.n, d.cases, d.missing, d.sum), (10.0, 10, 0, 55.0));
        assert!(close(d.mean, 5.5) && close(d.variance, 9.166_666_666_666_666) && close(d.sd, 3.027_650_354_097_491_7));
        assert!(close(d.se_mean, 0.957_427_107_756_338_1));
        assert!(close(d.skewness, 0.0) && close(d.kurtosis, -1.2));
        assert!(close(d.se_skewness, 0.687_042_918_621_516_7) && close(d.se_kurtosis, 1.334_248_769_989_982));
        assert!(close(d.q1, 3.25) && close(d.median, 5.5) && close(d.q3, 7.75) && close(d.iqr, 4.5) && close(d.range, 9.0));
        assert!(close(d.percentiles[0].value, 1.45) && close(d.percentiles[1].value, 9.55));
        assert!(close(d.cv, 3.027_650_354_097_491_7 / 5.5));
        let skewed = describe(plain(&[1.0, 2.0, 3.0, 10.0]), &[], 7).unwrap();
        assert!(close(skewed.skewness, 1.763_632_614_803_888) && close(skewed.kurtosis, 3.228));
    }

    /// NumPy's `percentile` methods, which are Hyndman & Fan's types 1–9 in order, on 1..=10.
    #[test]
    fn every_hyndman_fan_type() {
        let xs: Vec<(f64, f64)> = (1..=10).map(|x| (f64::from(x), 1.0)).collect();
        let expected: [[f64; 5]; 9] = [
            [1.0, 3.0, 5.0, 8.0, 10.0],
            [1.0, 3.0, 5.5, 8.0, 10.0],
            [1.0, 2.0, 5.0, 8.0, 10.0],
            [1.0, 2.5, 5.0, 7.5, 9.5],
            [1.0, 3.0, 5.5, 8.0, 10.0],
            [1.0, 2.75, 5.5, 8.25, 10.0],
            [1.45, 3.25, 5.5, 7.75, 9.55],
            [1.0, 2.916_666_666_666_666_5, 5.5, 8.083_333_333_333_334, 10.0],
            [1.0, 2.9375, 5.5, 8.0625, 10.0],
        ];
        for (t, row) in expected.iter().enumerate() {
            for (p, want) in [0.05, 0.25, 0.5, 0.75, 0.95].iter().zip(row) {
                let got = quantile(&xs, 10.0, *p, t as u8 + 1);
                assert!(close(got, *want), "type {} p {p}: {got:?}, not {want}", t + 1);
            }
        }
    }

    /// Whole weights are repeated cases: 2×3, 5×1, 9×2 is 2, 2, 2, 5, 9, 9.
    #[test]
    fn frequency_weights_repeat_cases() {
        let weighted = describe([(Some(2.0), 3.0), (Some(5.0), 1.0), (Some(9.0), 2.0)], &[], 7).unwrap();
        let repeated = describe(plain(&[2.0, 2.0, 2.0, 5.0, 9.0, 9.0]), &[], 7).unwrap();
        assert_eq!(weighted.n, 6.0);
        assert_eq!(weighted.cases, 3);
        for (a, b) in [
            (weighted.mean, repeated.mean),
            (weighted.variance, repeated.variance),
            (weighted.skewness, repeated.skewness),
            (weighted.kurtosis, repeated.kurtosis),
            (weighted.q1, repeated.q1),
            (weighted.median, repeated.median),
            (weighted.q3, repeated.q3),
        ] {
            assert!(close(a, b.unwrap()), "{a:?} vs {b:?}");
        }
        // NumPy on the repeated cases: mean 4.8333, variance 11.7667, G1 0.56818, G2 −2.2248.
        assert!(close(weighted.variance, 11.766_666_666_666_667) && close(weighted.skewness, 0.568_181_393_417_509_4));
        assert!(close(weighted.kurtosis, -2.224_799_171_809_419_6));
        assert!(close(weighted.median, 3.5) && close(weighted.q3, 8.0));
    }

    #[test]
    fn too_few_cases_and_bad_input() {
        let d = describe([(Some(4.0), 1.0), (None, 1.0), (Some(1.0), 0.0), (Some(f64::NAN), 1.0), (Some(2.0), -1.0)], &[50.0], 7).unwrap();
        assert_eq!((d.cases, d.missing, d.mean, d.sd, d.skewness, d.kurtosis), (1, 4, Some(4.0), None, None, None));
        assert_eq!(d.percentiles, vec![Percentile { p: 50.0, value: Some(4.0) }]);
        let none = describe(Vec::<(Option<f64>, f64)>::new(), &[], 7).unwrap();
        assert_eq!((none.n, none.mean, none.median, none.sum.is_sign_positive()), (0.0, None, None, true));
        let flat = describe(plain(&[3.0, 3.0, 3.0, 3.0]), &[], 7).unwrap();
        assert_eq!((flat.sd, flat.skewness, flat.kurtosis, flat.cv), (Some(0.0), None, None, Some(0.0)));
        assert!(describe(plain(&[1.0]), &[101.0], 7).is_err());
        assert!(describe(plain(&[1.0]), &[], 0).is_err() && describe(plain(&[1.0]), &[], 10).is_err());
    }
}
