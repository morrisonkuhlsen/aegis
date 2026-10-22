//! Descriptive statistics: a field's summary, frequency tables, cross-tabulations and
//! correlations, with their definitions stated, since statistical packages differ on them.
//!
//! - **Variance and SD are the sample ones** (divisor n − 1); the standard error of the mean is
//!   SD / √n; CV = SD / |mean|.
//! - **Skewness** is the adjusted Fisher–Pearson G1 = √(n(n − 1)) / (n − 2) · m₃ / m₂^(3/2), and
//!   **kurtosis** the adjusted excess G2 = (n − 1) / ((n − 2)(n − 3)) · ((n + 1)(m₄ / m₂² − 3) + 6),
//!   with mₖ the k-th central moment (divisor n); both are what SPSS and Excel report (Joanes
//!   and Gill 1998, types 2).
//! - **Quantiles** follow Hyndman and Fan (1996): type 7 by default (linear interpolation of
//!   order statistics, R's and NumPy's default); any of their types 1–9 can be asked for (type 6
//!   is SPSS's and Minitab's).
//! - **Weights** are frequency weights: n is Σw, the mean Σwx / Σw, the variance
//!   Σw(x − mean)² / (Σw − 1). Quantiles, skewness and kurtosis are unweighted and say so.
//! - **Cross-tabulation**: Pearson's χ² of independence, its degrees of freedom (r − 1)(c − 1)
//!   and p-value, Cramér's V = √(χ² / (N · (min(r, c) − 1))), and how many expected counts are
//!   below 5 (when χ²'s approximation is doubtful).
//! - **Correlation**: Pearson's r, or Spearman's ρ (Pearson's r of the ranks, ties given their
//!   mean rank), on the rows where both values are present, with a two-sided p-value from
//!   t = r √((n − 2) / (1 − r²)) on n − 2 degrees of freedom.

use std::collections::BTreeMap;

use mkaegis_core::{Error, Result};
use serde::Serialize;

use crate::inference::{chi2_sf, t_two_sided_p};

/// A numeric field's summary.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    /// Values present (Σw with weights).
    pub n: f64,
    pub missing: usize,
    pub sum: f64,
    pub mean: Option<f64>,
    pub variance: Option<f64>,
    pub sd: Option<f64>,
    pub se_mean: Option<f64>,
    pub cv: Option<f64>,
    pub min: Option<f64>,
    pub q1: Option<f64>,
    pub median: Option<f64>,
    pub q3: Option<f64>,
    pub max: Option<f64>,
    pub iqr: Option<f64>,
    pub skewness: Option<f64>,
    pub kurtosis: Option<f64>,
    /// The requested percentiles, as (p in 0..=100, value).
    pub percentiles: Vec<(f64, f64)>,
    pub quantile_type: u8,
    pub weighted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Frequency {
    pub value: String,
    pub count: usize,
    pub percent: f64,
    pub cumulative_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrossTab {
    pub rows: Vec<String>,
    pub columns: Vec<String>,
    /// counts[row][column].
    pub counts: Vec<Vec<usize>>,
    pub total: usize,
    pub chi2: f64,
    pub df: usize,
    pub p: f64,
    pub cramers_v: f64,
    /// Expected counts below 5, of all cells.
    pub low_expected: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Correlation {
    pub method: &'static str,
    pub fields: Vec<String>,
    /// r[i][j]; NaN where fewer than 3 rows have both values or one is constant.
    pub r: Vec<Vec<f64>>,
    pub n: Vec<Vec<usize>>,
    pub p: Vec<Vec<f64>>,
}

/// The summary of `values` (`None` for a missing value), with frequency `weights` if given, the
/// `percentiles` (0–100) asked for, by quantile `kind` (Hyndman–Fan type 1–9).
pub fn summarize(values: &[Option<f64>], weights: Option<&[Option<f64>]>, percentiles: &[f64], kind: u8) -> Result<Summary> {
    if !(1..=9).contains(&kind) {
        return Err(Error::param("quantileType", "a Hyndman–Fan type from 1 to 9"));
    }
    if let Some(p) = percentiles.iter().find(|p| !(0.0..=100.0).contains(*p)) {
        return Err(Error::param("percentiles", format!("{p} is not between 0 and 100")));
    }
    // A row counts when its value (and weight) is present; a weight must be ≥ 0.
    let mut pairs: Vec<(f64, f64)> = Vec::with_capacity(values.len());
    let mut missing = 0;
    for (i, v) in values.iter().enumerate() {
        let w = match weights {
            None => Some(1.0),
            Some(ws) => ws.get(i).copied().flatten(),
        };
        match (v.filter(|x| x.is_finite()), w.filter(|w| w.is_finite())) {
            (Some(_), Some(w)) if w < 0.0 => return Err(Error::param("weightField", format!("row {} has a negative weight", i + 1))),
            (Some(x), Some(w)) => pairs.push((x, w)),
            _ => missing += 1,
        }
    }
    let mut sorted: Vec<f64> = pairs.iter().map(|p| p.0).collect();
    sorted.sort_by(f64::total_cmp);
    let k = sorted.len();
    let n: f64 = pairs.iter().map(|p| p.1).sum();
    let sum: f64 = pairs.iter().fold(0.0, |a, (x, w)| a + w * x);
    let mean = (n > 0.0).then(|| sum / n);
    let variance = mean.filter(|_| n > 1.0).map(|m| pairs.iter().map(|(x, w)| w * (x - m).powi(2)).sum::<f64>() / (n - 1.0));
    let sd = variance.map(f64::sqrt);
    let q = |p: f64| (k > 0).then(|| quantile(&sorted, p, kind));
    // Shape, unweighted, from the central moments.
    let kf = k as f64;
    let plain_mean = (k > 0).then(|| sorted.iter().sum::<f64>() / kf);
    let moment = |r: i32| plain_mean.map(|m| sorted.iter().map(|x| (x - m).powi(r)).sum::<f64>() / kf);
    let (m2, m3, m4) = (moment(2), moment(3), moment(4));
    let skewness = match (m2, m3) {
        (Some(m2), Some(m3)) if k > 2 && m2 > 0.0 => Some((kf * (kf - 1.0)).sqrt() / (kf - 2.0) * m3 / m2.powf(1.5)),
        _ => None,
    };
    let kurtosis = match (m2, m4) {
        (Some(m2), Some(m4)) if k > 3 && m2 > 0.0 => Some((kf - 1.0) / ((kf - 2.0) * (kf - 3.0)) * ((kf + 1.0) * (m4 / (m2 * m2) - 3.0) + 6.0)),
        _ => None,
    };
    let (q1, q3) = (q(0.25), q(0.75));
    Ok(Summary {
        n,
        missing,
        // From +0: an empty sum is −0, which would print as `-0.0` (#4).
        sum: sum + 0.0,
        mean,
        variance,
        sd,
        se_mean: sd.map(|s| s / n.sqrt()),
        cv: sd.zip(mean).filter(|(_, m)| *m != 0.0).map(|(s, m)| s / m.abs()),
        min: sorted.first().copied(),
        q1,
        median: q(0.5),
        q3,
        max: sorted.last().copied(),
        iqr: q1.zip(q3).map(|(a, b)| b - a),
        skewness,
        kurtosis,
        percentiles: percentiles.iter().filter_map(|p| Some((*p, q(p / 100.0)?))).collect(),
        quantile_type: kind,
        weighted: weights.is_some(),
    })
}

/// Q(p) of sorted values, by Hyndman and Fan's type `kind` (1–9); 0-based order statistics.
pub fn quantile(sorted: &[f64], p: f64, kind: u8) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return f64::NAN;
    }
    let nf = n as f64;
    let p = p.clamp(0.0, 1.0);
    // The 1-based order statistic x₍ⱼ₎, clamped to the sample.
    let x = |j: f64| sorted.get((j.clamp(1.0, nf) as usize).saturating_sub(1)).copied().unwrap_or(f64::NAN);
    match kind {
        // Discontinuous: the inverse of the empirical distribution, averaged at jumps, nearest even.
        1..=3 => {
            let np = nf * p;
            match kind {
                1 => x(np.ceil().max(1.0)),
                2 => {
                    let j = np.floor();
                    if np == j { (x(j) + x(j + 1.0)) / 2.0 } else { x(j + 1.0) }
                }
                // Type 3: the nearest order statistic, the even one on a tie.
                _ => x((np - 0.5).round_ties_even().max(1.0)),
            }
        }
        // Continuous: h = (n + m)p + c, interpolating between x₍⌊h⌋₎ and x₍⌊h⌋+1₎.
        _ => {
            let (a, b) = match kind {
                4 => (0.0, 1.0),
                5 => (0.5, 0.5),
                6 => (0.0, 0.0),
                7 => (1.0, 1.0),
                8 => (1.0 / 3.0, 1.0 / 3.0),
                _ => (3.0 / 8.0, 3.0 / 8.0),
            };
            let h = (nf + 1.0 - a - b) * p + a;
            let j = h.floor();
            x(j) + (h - j) * (x(j + 1.0) - x(j))
        }
    }
}

/// The frequency table of a field's values (`None` is "NULL"), most frequent first, ties by value.
pub fn frequencies(values: &[Option<String>]) -> Vec<Frequency> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for v in values {
        *counts.entry(v.clone().unwrap_or_else(|| "NULL".into())).or_insert(0) += 1;
    }
    let total = values.len().max(1) as f64;
    let mut rows: Vec<(String, usize)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut cumulative = 0.0;
    rows.into_iter()
        .map(|(value, count)| {
            let percent = 100.0 * count as f64 / total;
            cumulative += percent;
            Frequency { value, count, percent, cumulative_percent: cumulative }
        })
        .collect()
}

/// The cross-tabulation of two categorical fields over the rows where both are present.
pub fn cross_tab(rows: &[Option<String>], columns: &[Option<String>]) -> Result<CrossTab> {
    let pairs: Vec<(&String, &String)> = rows.iter().zip(columns).filter_map(|(r, c)| Some((r.as_ref()?, c.as_ref()?))).collect();
    let mut row_names: Vec<String> = pairs.iter().map(|p| p.0.clone()).collect();
    let mut col_names: Vec<String> = pairs.iter().map(|p| p.1.clone()).collect();
    row_names.sort();
    row_names.dedup();
    col_names.sort();
    col_names.dedup();
    if row_names.len() > 1_000 || col_names.len() > 1_000 {
        return Err(Error::param("field", "more than 1,000 categories: is this a categorical field?"));
    }
    if row_names.len() < 2 || col_names.len() < 2 {
        return Err(Error::param("field", "each field needs at least two categories"));
    }
    let mut counts = vec![vec![0usize; col_names.len()]; row_names.len()];
    for (r, c) in &pairs {
        if let (Ok(i), Ok(j)) = (row_names.binary_search(r), col_names.binary_search(c))
            && let Some(cell) = counts.get_mut(i).and_then(|row| row.get_mut(j))
        {
            *cell += 1;
        }
    }
    let total = pairs.len();
    let tf = total as f64;
    let row_sums: Vec<f64> = counts.iter().map(|r| r.iter().sum::<usize>() as f64).collect();
    let col_sums: Vec<f64> = (0..col_names.len()).map(|j| counts.iter().map(|r| r.get(j).copied().unwrap_or(0)).sum::<usize>() as f64).collect();
    let (mut chi2, mut low_expected) = (0.0, 0);
    for (i, row) in counts.iter().enumerate() {
        for (j, o) in row.iter().enumerate() {
            let e = row_sums.get(i).copied().unwrap_or(0.0) * col_sums.get(j).copied().unwrap_or(0.0) / tf;
            if e < 5.0 {
                low_expected += 1;
            }
            if e > 0.0 {
                chi2 += (*o as f64 - e).powi(2) / e;
            }
        }
    }
    let df = (row_names.len() - 1) * (col_names.len() - 1);
    let m = (row_names.len().min(col_names.len()) - 1) as f64;
    Ok(CrossTab {
        rows: row_names,
        columns: col_names,
        counts,
        total,
        chi2,
        df,
        p: chi2_sf(chi2, df as f64),
        cramers_v: (chi2 / (tf * m)).sqrt(),
        low_expected,
    })
}

/// The correlation matrix of `columns` (named `fields`), Pearson or Spearman, pairwise complete.
pub fn correlation(fields: &[String], columns: &[Vec<Option<f64>>], spearman: bool) -> Result<Correlation> {
    if fields.len() < 2 || fields.len() != columns.len() {
        return Err(Error::param("fields", "two numeric fields or more"));
    }
    let k = fields.len();
    let (mut r, mut n, mut p) = (vec![vec![f64::NAN; k]; k], vec![vec![0usize; k]; k], vec![vec![f64::NAN; k]; k]);
    for i in 0..k {
        for j in i..k {
            let (a, b) = (&columns[i], &columns[j]);
            let pairs: Vec<(f64, f64)> =
                a.iter().zip(b).filter_map(|(x, y)| Some((x.filter(|v| v.is_finite())?, y.filter(|v| v.is_finite())?))).collect();
            let (xs, ys): (Vec<f64>, Vec<f64>) = pairs.into_iter().unzip();
            let (xs, ys) = if spearman { (ranks(&xs), ranks(&ys)) } else { (xs, ys) };
            let m = xs.len();
            let value = pearson(&xs, &ys);
            let pv = value
                .filter(|_| m > 2)
                .map(|v| if v.abs() >= 1.0 { 0.0 } else { t_two_sided_p(v * ((m as f64 - 2.0) / (1.0 - v * v)).sqrt(), m as f64 - 2.0) });
            for (a, b) in [(i, j), (j, i)] {
                r[a][b] = value.unwrap_or(f64::NAN);
                n[a][b] = m;
                p[a][b] = if a == b { 0.0 } else { pv.unwrap_or(f64::NAN) };
            }
        }
    }
    Ok(Correlation { method: if spearman { "spearman" } else { "pearson" }, fields: fields.to_vec(), r, n, p })
}

fn pearson(x: &[f64], y: &[f64]) -> Option<f64> {
    let n = x.len();
    if n < 3 {
        return None;
    }
    let (mx, my) = (x.iter().sum::<f64>() / n as f64, y.iter().sum::<f64>() / n as f64);
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        sxx += (a - mx).powi(2);
        syy += (b - my).powi(2);
        sxy += (a - mx) * (b - my);
    }
    (sxx > 0.0 && syy > 0.0).then(|| (sxy / (sxx * syy).sqrt()).clamp(-1.0, 1.0))
}

/// 1-based ranks, ties given their mean rank.
fn ranks(v: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..v.len()).collect();
    order.sort_by(|a, b| v[*a].total_cmp(&v[*b]));
    let mut out = vec![0.0; v.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && v[order[j + 1]] == v[order[i]] {
            j += 1;
        }
        let rank = (i + j) as f64 / 2.0 + 1.0;
        for o in &order[i..=j] {
            out[*o] = rank;
        }
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: [f64; 8] = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];

    fn some(v: &[f64]) -> Vec<Option<f64>> {
        v.iter().map(|x| Some(*x)).collect()
    }

    /// V by hand: n 8, sum 40, mean 5; squared deviations sum to 32, so the sample variance is
    /// 32/7 and SD √(32/7); m₂ = 4, m₃ = 42/8 = 5.25, m₄ = 330/8 = 41.25. G1 = √56/6 · 5.25/8 =
    /// 0.818487…; G2 = 7/30 · (9·(41.25/16 − 3) + 6) = 0.940625. Type 7: Q1 at h = 2.75 is 4,
    /// the median 4.5, Q3 at h = 6.25 is 5.5.
    #[test]
    fn summary_by_hand() {
        let s = summarize(&some(&V), None, &[10.0, 90.0], 7).unwrap();
        assert_eq!((s.n, s.missing, s.sum, s.mean, s.min, s.max), (8.0, 0, 40.0, Some(5.0), Some(2.0), Some(9.0)));
        assert!((s.variance.unwrap() - 32.0 / 7.0).abs() < 1e-14);
        assert!((s.se_mean.unwrap() - (32.0f64 / 7.0).sqrt() / 8f64.sqrt()).abs() < 1e-14);
        assert!((s.skewness.unwrap() - 56f64.sqrt() / 6.0 * 5.25 / 8.0).abs() < 1e-12, "{:?}", s.skewness);
        assert!((s.kurtosis.unwrap() - 0.940_625).abs() < 1e-12, "{:?}", s.kurtosis);
        assert_eq!((s.q1, s.median, s.q3, s.iqr), (Some(4.0), Some(4.5), Some(5.5), Some(1.5)));
        // P10 at h = 1.7: 2 + 0.7·2 = 3.4; P90 at h = 7.3: 7 + 0.3·2 = 7.6.
        let p: Vec<f64> = s.percentiles.iter().map(|p| p.1).collect();
        assert!((p[0] - 3.4).abs() < 1e-12 && (p[1] - 7.6).abs() < 1e-12, "{p:?}");
    }

    /// The nine types on 1..=10 for p = 0.25, by hand from Hyndman and Fan's definitions:
    /// 1: ⌈np⌉ = 3 → 3; 2: np = 2.5 isn't whole → x₍₃₎ = 3; 3: np − ½ = 2 → 2; then h for the
    /// continuous types, h = (n + 1 − a − b)p + a: 4: 2.5; 5: 3; 6: 2.75; 7: 3.25;
    /// 8: (10 + ⅓)/4 + ⅓ = 2.916…; 9: (10 + ¼)/4 + ⅜ = 2.9375.
    #[test]
    fn hyndman_fan_types() {
        let s: Vec<f64> = (1..=10).map(f64::from).collect();
        let expected = [3.0, 3.0, 2.0, 2.5, 3.0, 2.75, 3.25, 31.0 / 12.0 + 1.0 / 3.0, 2.9375];
        for (kind, want) in (1..=9).zip(expected) {
            assert!((quantile(&s, 0.25, kind) - want).abs() < 1e-12, "type {kind}: {} vs {want}", quantile(&s, 0.25, kind));
        }
        // np − ½ = 1.5 rounds to the even 2 (type 3), and 0 and 1 give the extremes.
        assert_eq!(quantile(&s, 0.2, 3), 2.0);
        assert_eq!((quantile(&s, 0.0, 7), quantile(&s, 1.0, 7)), (1.0, 10.0));
        assert!(summarize(&some(&V), None, &[], 10).is_err());
        assert!(summarize(&some(&V), None, &[101.0], 7).is_err());
    }

    #[test]
    fn weights_and_missing_values() {
        // Weights 1, 2, 1 on 1, 2, 3 are the sample 1, 2, 2, 3: mean 2, variance 2/3.
        let w = summarize(&some(&[1.0, 2.0, 3.0]), Some(&some(&[1.0, 2.0, 1.0])), &[], 7).unwrap();
        assert_eq!((w.n, w.mean, w.weighted), (4.0, Some(2.0), true));
        assert!((w.variance.unwrap() - 2.0 / 3.0).abs() < 1e-15);
        let m = summarize(&[Some(1.0), None, Some(3.0), Some(f64::NAN)], None, &[], 7).unwrap();
        assert_eq!((m.n, m.missing, m.mean), (2.0, 2, Some(2.0)));
        assert!(summarize(&some(&[1.0, 2.0]), Some(&some(&[1.0, -1.0])), &[], 7).is_err());
        let empty = summarize(&[None], None, &[], 7).unwrap();
        assert!(empty.mean.is_none() && empty.sum.is_sign_positive());
    }

    /// A 2 × 2 table, by hand: a/b by x/y with counts 10, 20 / 30, 40. Row sums 30, 70; column
    /// sums 40, 60; N 100. Expected 12, 18 / 28, 42, so χ² = 4/12 + 4/18 + 4/28 + 4/42 = 0.793651,
    /// df 1, p = erfc(√(χ²/2)), V = √(χ²/100).
    #[test]
    fn cross_tabulation_by_hand() {
        let mut rows = Vec::new();
        let mut cols = Vec::new();
        for (r, c, n) in [("a", "x", 10), ("a", "y", 20), ("b", "x", 30), ("b", "y", 40)] {
            for _ in 0..n {
                rows.push(Some(r.to_string()));
                cols.push(Some(c.to_string()));
            }
        }
        rows.push(None);
        cols.push(Some("x".into()));
        let t = cross_tab(&rows, &cols).unwrap();
        let chi2 = 4.0 / 12.0 + 4.0 / 18.0 + 4.0 / 28.0 + 4.0 / 42.0;
        assert_eq!((t.total, t.df, t.counts.clone(), t.low_expected), (100, 1, vec![vec![10, 20], vec![30, 40]], 0));
        assert!((t.chi2 - chi2).abs() < 1e-12);
        assert!((t.cramers_v - (chi2 / 100.0).sqrt()).abs() < 1e-12);
        assert!((t.p - chi2_sf(chi2, 1.0)).abs() < 1e-15 && t.p > 0.3 && t.p < 0.4);
        assert!(cross_tab(&[Some("a".into())], &[Some("x".into())]).is_err());
    }

    #[test]
    fn frequencies_and_correlations() {
        let f = frequencies(&[Some("b".into()), Some("a".into()), Some("b".into()), None]);
        assert_eq!(f.iter().map(|r| (r.value.as_str(), r.count)).collect::<Vec<_>>(), [("b", 2), ("NULL", 1), ("a", 1)]);
        assert_eq!(f.last().map(|r| r.cumulative_percent), Some(100.0));
        // y = x² on 1..5: Pearson 0.981, Spearman exactly 1; a reversed copy correlates −1.
        let x = some(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let y = some(&[1.0, 4.0, 9.0, 16.0, 25.0]);
        let z = some(&[5.0, 4.0, 3.0, 2.0, 1.0]);
        let names = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        let p = correlation(&names, &[x.clone(), y.clone(), z.clone()], false).unwrap();
        // By hand: Sxy = 60, Sxx = 10, Syy = 374, r = 60 / √3740.
        assert!((p.r[0][1] - 60.0 / 3740f64.sqrt()).abs() < 1e-12);
        assert_eq!((p.r[0][2], p.r[1][1], p.p[0][0]), (-1.0, 1.0, 0.0));
        // t = r √(3 / (1 − r²)) on 3 df.
        let r = p.r[0][1];
        assert!((p.p[0][1] - t_two_sided_p(r * (3.0 / (1.0 - r * r)).sqrt(), 3.0)).abs() < 1e-15);
        let s = correlation(&names, &[x, y, z], true).unwrap();
        assert_eq!((s.r[0][1], s.method), (1.0, "spearman"));
        assert_eq!(ranks(&[3.0, 1.0, 3.0, 2.0]), vec![3.5, 1.0, 3.5, 2.0]);
        assert!(correlation(&names[..1], &[some(&[1.0])], false).is_err());
    }
}
