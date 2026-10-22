//! Choropleth classification: where a numeric field's values are cut into classes, and how well
//! the classes fit them.
//!
//! - Equal interval and quantile.
//! - Natural breaks: Fisher's (1958) exact optimal partition, the classes with the smallest sum
//!   of squared deviations from their means (what Jenks's method approximates), by dynamic
//!   programming over the sorted values.
//! - Geometric interval: bounds in geometric progression from the minimum to the maximum, for
//!   positive, skewed values.
//! - Manual: the bounds given.
//! - Standard deviation map: six classes at the mean ± 1 and 2 standard deviations.
//! - Box map: six classes at the hinges, lower outliers below Q1 − h·IQR, upper ones above
//!   Q3 + h·IQR (h = 1.5, or 3 for extreme outliers).
//! - Percentile map: <1 %, 1–10 %, 10–50 %, 50–90 %, 90–99 %, >99 %.
//!
//! Quantiles are by linear interpolation between order statistics (Hyndman and Fan 1996,
//! definition 7): for p in 0..=1 and n sorted values, h = (n − 1)·p, and Q(p) = x₍⌊h⌋₎ +
//! (h − ⌊h⌋)(x₍⌊h⌋+1₎ − x₍⌊h⌋₎), 0-based. Every classification reports its goodness of variance
//! fit, GVF = 1 − SDCM / SDAM (Jenks 1977): 1 when the classes explain all the variance.

use mkaegis_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// The most values natural breaks partitions exactly; more are reduced to this many quantiles
/// first, and the result is marked approximate.
pub const MAX_EXACT: usize = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Classification {
    #[default]
    EqualInterval,
    Quantile,
    NaturalBreaks,
    GeometricInterval,
    Manual,
    StandardDeviation,
    BoxMap,
    Percentile,
}

impl Classification {
    pub fn parse(text: &str) -> Option<Classification> {
        match text.to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "" | "equalinterval" => Some(Classification::EqualInterval),
            "quantile" => Some(Classification::Quantile),
            "naturalbreaks" | "jenks" | "fisherjenks" => Some(Classification::NaturalBreaks),
            "geometricinterval" | "geometric" => Some(Classification::GeometricInterval),
            "manual" => Some(Classification::Manual),
            "standarddeviation" | "stddev" => Some(Classification::StandardDeviation),
            "boxmap" | "box" => Some(Classification::BoxMap),
            "percentile" | "percentilemap" => Some(Classification::Percentile),
            _ => None,
        }
    }

    /// Whether the number of classes is the method's own (six), not the caller's.
    pub fn fixed_classes(self) -> bool {
        matches!(self, Classification::StandardDeviation | Classification::BoxMap | Classification::Percentile)
    }
}

/// What a classification is asked for beyond its method.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    /// Classes, for the methods that take a number.
    pub classes: usize,
    /// Box map: the hinge multiple of the IQR (1.5 by default).
    pub hinge: Option<f64>,
    /// Manual: the upper bounds, ascending.
    pub breaks: Vec<f64>,
}

/// A classification: each class's upper bound (a value belongs to the first class whose bound
/// it doesn't exceed), labels for the methods whose classes have names, and the fit.
#[derive(Debug, Clone, PartialEq)]
pub struct Classes {
    pub breaks: Vec<f64>,
    /// Empty when classes are labelled by their range.
    pub labels: Vec<String>,
    pub gvf: f64,
    /// Natural breaks on more than [`MAX_EXACT`] values.
    pub approximate: bool,
}

/// Classifies `values` (non-finite ones are left out).
pub fn classify(values: &[f64], method: Classification, options: &Options) -> Result<Classes> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return Err(Error::param("field", "no numeric values to classify"));
    }
    v.sort_by(f64::total_cmp);
    let (min, max) = (v[0], v[v.len() - 1]);
    let k = options.classes.clamp(1, crate::symbology::MAX_CLASSES);
    let mut approximate = false;
    let mut labels = Vec::new();
    let breaks: Vec<f64> = match method {
        Classification::EqualInterval => (1..=k).map(|i| min + (max - min) * i as f64 / k as f64).collect(),
        Classification::Quantile => (1..=k).map(|i| v.get((i * v.len()).div_ceil(k).saturating_sub(1)).copied().unwrap_or(max)).collect(),
        Classification::NaturalBreaks => {
            let sample: Vec<f64> = if v.len() > MAX_EXACT {
                approximate = true;
                (0..MAX_EXACT).map(|i| quantile(&v, i as f64 / (MAX_EXACT - 1) as f64)).collect()
            } else {
                v.clone()
            };
            fisher(&sample, k)
        }
        Classification::GeometricInterval => {
            if min <= 0.0 {
                return Err(Error::param("method", "geometric intervals need values above 0"));
            }
            let r = (max / min).powf(1.0 / k as f64);
            (1..=k).map(|i| if i == k { max } else { min * r.powi(i as i32) }).collect()
        }
        Classification::Manual => {
            let b = &options.breaks;
            if b.is_empty() || b.iter().any(|x| !x.is_finite()) || b.windows(2).any(|w| w[0] >= w[1]) {
                return Err(Error::param("breaks", "finite upper bounds in ascending order"));
            }
            let mut b: Vec<f64> = b.iter().copied().filter(|x| *x < max).collect();
            b.push(max);
            b
        }
        Classification::StandardDeviation => {
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
            labels = ["< −2 SD", "−2 to −1 SD", "−1 SD to mean", "mean to +1 SD", "+1 to +2 SD", "> +2 SD"].map(String::from).to_vec();
            vec![mean - 2.0 * sd, mean - sd, mean, mean + sd, mean + 2.0 * sd, max.max(mean + 2.0 * sd)]
        }
        Classification::BoxMap => {
            let h = options.hinge.unwrap_or(1.5);
            if !(h.is_finite() && h > 0.0) {
                return Err(Error::param("hinge", "a positive multiple of the interquartile range (1.5 or 3)"));
            }
            let (q1, q2, q3) = (quantile(&v, 0.25), quantile(&v, 0.5), quantile(&v, 0.75));
            let iqr = q3 - q1;
            labels = ["Lower outlier", "< 25%", "25–50%", "50–75%", "> 75%", "Upper outlier"].map(String::from).to_vec();
            // Outliers are strictly beyond the fences: the bound just below the lower fence.
            let lower = (q1 - h * iqr).next_down();
            let upper = q3 + h * iqr;
            vec![lower, q1, q2, q3, upper, max.max(upper)]
        }
        Classification::Percentile => {
            labels = ["< 1%", "1–10%", "10–50%", "50–90%", "90–99%", "> 99%"].map(String::from).to_vec();
            let q = |p: f64| quantile(&v, p);
            vec![q(0.01), q(0.10), q(0.50), q(0.90), q(0.99), max]
        }
    };
    let mut breaks = breaks;
    if labels.is_empty() {
        // Range-labelled classes: equal bounds would be empty classes with the same label.
        breaks.dedup();
        if let Some(last) = breaks.last_mut() {
            *last = max;
        }
    }
    let gvf = gvf(&v, &breaks);
    Ok(Classes { breaks, labels, gvf, approximate })
}

/// The class of `x` among upper bounds `breaks`: the first it doesn't exceed, else the last.
pub fn class_of(breaks: &[f64], x: f64) -> usize {
    breaks.iter().position(|b| x <= *b).unwrap_or(breaks.len().saturating_sub(1))
}

/// The goodness of variance fit of sorted `v` cut at `breaks`.
pub fn gvf(v: &[f64], breaks: &[f64]) -> f64 {
    let ssd = |xs: &[f64]| {
        if xs.is_empty() {
            return 0.0;
        }
        let m = xs.iter().sum::<f64>() / xs.len() as f64;
        xs.iter().map(|x| (x - m).powi(2)).sum::<f64>()
    };
    let total = ssd(v);
    if total == 0.0 {
        return 1.0;
    }
    let mut within = 0.0;
    let mut start = 0;
    for (c, _) in breaks.iter().enumerate() {
        let end = v.iter().skip(start).position(|x| class_of(breaks, *x) != c).map_or(v.len(), |e| start + e);
        within += ssd(v.get(start..end).unwrap_or_default());
        start = end;
    }
    1.0 - within / total
}

/// Q(p) of sorted values, by linear interpolation (Hyndman and Fan's definition 7).
pub fn quantile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return f64::NAN;
    }
    let h = (n - 1) as f64 * p.clamp(0.0, 1.0);
    let lo = h.floor() as usize;
    let a = sorted.get(lo).copied().unwrap_or(0.0);
    let b = sorted.get(lo + 1).copied().unwrap_or(a);
    a + (h - lo as f64) * (b - a)
}

/// Fisher's exact optimal partition of sorted `v` into `k` classes, as upper bounds. cost(i, j)
/// is the sum of squared deviations of v[i..=j], from prefix sums; best[c][j] the least total
/// cost of v[0..=j] in c + 1 classes. O(k·n²) time, O(k·n) memory.
fn fisher(v: &[f64], k: usize) -> Vec<f64> {
    let n = v.len();
    let k = k.min(n).max(1);
    let (mut s1, mut s2) = (vec![0.0; n + 1], vec![0.0; n + 1]);
    for (i, x) in v.iter().enumerate() {
        s1[i + 1] = s1[i] + x;
        s2[i + 1] = s2[i] + x * x;
    }
    let cost = |i: usize, j: usize| {
        let m = (j - i + 1) as f64;
        let s = s1[j + 1] - s1[i];
        (s2[j + 1] - s2[i] - s * s / m).max(0.0)
    };
    let mut best = vec![vec![f64::INFINITY; n]; k];
    let mut from = vec![vec![0usize; n]; k];
    best[0] = (0..n).map(|j| cost(0, j)).collect();
    for c in 1..k {
        for j in c..n {
            for i in c..=j {
                let total = best[c - 1][i - 1] + cost(i, j);
                if total < best[c][j] {
                    best[c][j] = total;
                    from[c][j] = i;
                }
            }
        }
    }
    let mut bounds = vec![v[n - 1]];
    let mut j = n - 1;
    for c in (1..k).rev() {
        let i = from[c][j];
        bounds.push(v[i - 1]);
        j = i - 1;
    }
    bounds.reverse();
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKEWED: [f64; 12] = [1.0, 2.0, 2.5, 3.0, 9.0, 10.0, 11.5, 12.0, 30.0, 31.0, 55.0, 60.0];

    mod natural_breaks {
        use super::*;

        /// Fisher's partition against every way to cut the values into k classes.
        #[test]
        fn match_every_partition() {
            let options = |k| Options { classes: k, ..Options::default() };
            for k in 2..=5 {
                let got = classify(&SKEWED, Classification::NaturalBreaks, &options(k)).unwrap();
                let best = brute_force(&SKEWED, k);
                let ssd = |breaks: &[f64]| 1.0 - gvf(&SKEWED, breaks);
                assert!((ssd(&got.breaks) - ssd(&best)).abs() < 1e-12, "k {k}: {:?} vs {best:?}", got.breaks);
                assert_eq!(got.breaks.len(), k);
            }
            let three = classify(&SKEWED, Classification::NaturalBreaks, &options(3)).unwrap();
            assert_eq!(three.breaks, vec![12.0, 31.0, 60.0]);
            assert!(!three.approximate && three.gvf > 0.95);
        }

        #[test]
        fn many_values_are_approximate_but_sane() {
            let v: Vec<f64> = (0..10_000).map(|i| ((i * 7_919) % 10_000) as f64).collect();
            let c = classify(&v, Classification::NaturalBreaks, &Options { classes: 4, ..Options::default() }).unwrap();
            assert!(c.approximate && c.breaks.len() == 4 && c.gvf > 0.9);
        }

        /// Every choice of k − 1 cut points among the n − 1 gaps, the least within-class sum.
        fn brute_force(v: &[f64], k: usize) -> Vec<f64> {
            let n = v.len();
            let mut best = (f64::INFINITY, Vec::new());
            let mut cuts: Vec<usize> = (1..k).collect();
            loop {
                let mut breaks: Vec<f64> = cuts.iter().map(|c| v[c - 1]).collect();
                breaks.push(v[n - 1]);
                let within = 1.0 - gvf(v, &breaks);
                if within < best.0 {
                    best = (within, breaks);
                }
                // The next combination, in lexicographic order.
                let Some(i) = (0..cuts.len()).rev().find(|&i| cuts[i] < n - (cuts.len() - i)) else { break };
                cuts[i] += 1;
                for j in i + 1..cuts.len() {
                    cuts[j] = cuts[j - 1] + 1;
                }
            }
            best.1
        }
    }

    /// Definition 7 by hand on SKEWED (n = 12): Q(0.25) at h = 2.75 is 2.5 + 0.75·0.5 = 2.875;
    /// Q(0.5) at h = 5.5 is 10.75; Q(0.75) at h = 8.25 is 30.25. IQR = 27.375, so the upper
    /// fence is 30.25 + 1.5·27.375 = 71.3125 (no upper outlier) and the lower one is negative.
    #[test]
    fn box_map_hinges() {
        let c = classify(&SKEWED, Classification::BoxMap, &Options::default()).unwrap();
        assert_eq!(&c.breaks[1..5], &[2.875, 10.75, 30.25, 71.3125]);
        assert!(c.breaks[0] < 2.875 - 1.5 * 27.375 + 1e-9 && c.breaks[0] > 2.875 - 1.5 * 27.375 - 1e-9);
        assert_eq!(c.labels.len(), 6);
        // An extreme value is an upper outlier; 1.5 IQR moves to 3 with the hinge.
        let mut v = SKEWED.to_vec();
        v.push(500.0);
        let c = classify(&v, Classification::BoxMap, &Options::default()).unwrap();
        assert_eq!(class_of(&c.breaks, 500.0), 5);
        assert_eq!(class_of(&c.breaks, 60.0), 4);
        assert!(classify(&v, Classification::BoxMap, &Options { hinge: Some(-1.0), ..Options::default() }).is_err());
    }

    #[test]
    fn percentile_and_standard_deviation_maps() {
        let v: Vec<f64> = (1..=1000).map(f64::from).collect();
        let p = classify(&v, Classification::Percentile, &Options::default()).unwrap();
        // Q(0.01) at h = 9.99 is 10.99; Q(0.5) at h = 499.5 is 500.5.
        assert!((p.breaks[0] - 10.99).abs() < 1e-9 && (p.breaks[2] - 500.5).abs() < 1e-9);
        assert_eq!(class_of(&p.breaks, 1000.0), 5);
        let s = classify(&[1.0, 2.0, 3.0, 4.0, 5.0], Classification::StandardDeviation, &Options::default()).unwrap();
        let sd = 2f64.sqrt();
        assert_eq!(s.breaks[2], 3.0);
        assert!((s.breaks[3] - (3.0 + sd)).abs() < 1e-12 && s.labels[5] == "> +2 SD");
    }

    #[test]
    fn geometric_manual_and_gvf() {
        let g = classify(&[1.0, 10.0, 100.0, 1000.0], Classification::GeometricInterval, &Options { classes: 3, ..Options::default() }).unwrap();
        assert!((g.breaks[0] - 10.0).abs() < 1e-9 && (g.breaks[1] - 100.0).abs() < 1e-9 && g.breaks[2] == 1000.0);
        assert!(classify(&[0.0, 1.0], Classification::GeometricInterval, &Options { classes: 2, ..Options::default() }).is_err());
        let m = classify(&SKEWED, Classification::Manual, &Options { breaks: vec![5.0, 20.0, 100.0], ..Options::default() }).unwrap();
        assert_eq!(m.breaks, vec![5.0, 20.0, 60.0]);
        assert!(classify(&SKEWED, Classification::Manual, &Options { breaks: vec![20.0, 5.0], ..Options::default() }).is_err());
        // One class explains nothing; one class per value explains everything.
        assert_eq!(gvf(&SKEWED, &[60.0]), 0.0);
        assert!((gvf(&SKEWED, &SKEWED) - 1.0).abs() < 1e-12);
    }
}
