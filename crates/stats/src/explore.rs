//! Exploratory views' statistics: histogram bins, the box plot, an ordinary least-squares line and
//! the Moran scatter plot. The views draw them and brush features through them.
//!
//! - Bins: Sturges (1926), k = ⌈log₂ n⌉ + 1; Freedman and Diaconis (1981), width 2·IQR·n^(−1/3);
//!   or a given count. Bins are equal-width from the minimum to the maximum; a value on an inner
//!   edge belongs to the bin above, the maximum to the last bin.
//! - Box plot (Tukey 1977): the quartiles by linear interpolation (Hyndman and Fan's definition
//!   7, as the box map uses), fences at Q1 − h·IQR and Q3 + h·IQR, whiskers at the most extreme
//!   values inside the fences, and the rows beyond them as outliers.
//! - Least squares: y = a + b·x, with Pearson's r and R² = r².
//! - Moran scatter plot (Anselin 1996): each feature's standardised value z against its spatial
//!   lag Wz. With row-standardised weights the least-squares slope through the origin is Moran's I.

use mkaegis_core::{Error, Result};
use mkaegis_model::classify::quantile;
use serde::Serialize;

/// The most bins a histogram draws.
pub const MAX_BINS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bins {
    Sturges,
    FreedmanDiaconis,
    Count(usize),
}

impl Bins {
    /// `sturges`, `fd` (or `freedman-diaconis`), or a number of bins.
    pub fn parse(text: &str) -> Option<Bins> {
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "sturges" => Some(Bins::Sturges),
            "fd" | "freedman-diaconis" | "freedmandiaconis" => Some(Bins::FreedmanDiaconis),
            n => n.parse::<usize>().ok().filter(|n| (1..=MAX_BINS).contains(n)).map(Bins::Count),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Histogram {
    /// k + 1 edges for k bins.
    pub edges: Vec<f64>,
    pub counts: Vec<usize>,
}

impl Histogram {
    /// The bin of `x`, if it is within the histogram's range.
    pub fn bin_of(&self, x: f64) -> Option<usize> {
        let (first, last) = (*self.edges.first()?, *self.edges.last()?);
        if !(x >= first && x <= last) {
            return None;
        }
        let k = self.counts.len();
        let i = self.edges.iter().skip(1).position(|e| x < *e).unwrap_or(k - 1);
        Some(i.min(k - 1))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoxPlot {
    pub n: usize,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub hinge: f64,
    pub lower_fence: f64,
    pub upper_fence: f64,
    /// The most extreme values inside the fences.
    pub lower_whisker: f64,
    pub upper_whisker: f64,
    pub mean: f64,
    /// Rows (of the values given) beyond a fence.
    pub outliers: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fit {
    pub n: usize,
    pub intercept: f64,
    pub slope: f64,
    /// Pearson's correlation.
    pub r: f64,
    pub r2: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MoranScatter {
    /// Standardised values (mean 0, population standard deviation 1), by row.
    pub z: Vec<f64>,
    /// Their spatial lag Σⱼ wᵢⱼ zⱼ, by row.
    pub lag: Vec<f64>,
    /// The least-squares slope of the lag on z through the origin: Moran's I with
    /// row-standardised weights.
    pub slope: f64,
}

/// A histogram of the finite `values`.
pub fn histogram(values: &[f64], bins: Bins) -> Result<Histogram> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return Err(Error::param("field", "no numeric values"));
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let (min, max) = (v[0], v[n - 1]);
    let sturges = || (n as f64).log2().ceil() as usize + 1;
    let k = match bins {
        Bins::Count(k) => k,
        Bins::Sturges => sturges(),
        Bins::FreedmanDiaconis => {
            let width = 2.0 * (quantile(&v, 0.75) - quantile(&v, 0.25)) / (n as f64).cbrt();
            if width > 0.0 { ((max - min) / width).ceil() as usize } else { sturges() }
        }
    }
    .clamp(1, MAX_BINS);
    let k = if max > min { k } else { 1 };
    let edges: Vec<f64> = (0..=k).map(|i| if i == k { max } else { min + (max - min) * i as f64 / k as f64 }).collect();
    let mut h = Histogram { edges, counts: vec![0; k] };
    for x in &v {
        if let Some(c) = h.bin_of(*x).and_then(|b| h.counts.get_mut(b)) {
            *c += 1;
        }
    }
    Ok(h)
}

/// The box plot of `values` (rows; non-finite ones are left out) with fences at `hinge` × IQR.
pub fn box_plot(values: &[f64], hinge: f64) -> Result<BoxPlot> {
    if !(hinge.is_finite() && hinge > 0.0) {
        return Err(Error::param("hinge", "a positive multiple of the interquartile range (1.5 or 3)"));
    }
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return Err(Error::param("field", "no numeric values"));
    }
    v.sort_by(f64::total_cmp);
    let (q1, median, q3) = (quantile(&v, 0.25), quantile(&v, 0.5), quantile(&v, 0.75));
    let iqr = q3 - q1;
    let (lower_fence, upper_fence) = (q1 - hinge * iqr, q3 + hinge * iqr);
    let inside = v.iter().copied().filter(|x| *x >= lower_fence && *x <= upper_fence);
    let lower_whisker = inside.clone().fold(f64::INFINITY, f64::min);
    let upper_whisker = inside.fold(f64::NEG_INFINITY, f64::max);
    let outliers = values.iter().enumerate().filter(|(_, x)| x.is_finite() && (**x < lower_fence || **x > upper_fence)).map(|(i, _)| i).collect();
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    Ok(BoxPlot { n: v.len(), q1, median, q3, hinge, lower_fence, upper_fence, lower_whisker, upper_whisker, mean, outliers })
}

/// The least-squares line of `y` on `x`, over the pairs where both are finite.
pub fn ols(x: &[f64], y: &[f64]) -> Result<Fit> {
    let pairs: Vec<(f64, f64)> = x.iter().zip(y).map(|(a, b)| (*a, *b)).filter(|(a, b)| a.is_finite() && b.is_finite()).collect();
    let n = pairs.len();
    if n < 3 {
        return Err(Error::param("field", "needs at least 3 features with both values"));
    }
    let nf = n as f64;
    let (mx, my) = (pairs.iter().map(|p| p.0).sum::<f64>() / nf, pairs.iter().map(|p| p.1).sum::<f64>() / nf);
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for (a, b) in &pairs {
        sxx += (a - mx).powi(2);
        syy += (b - my).powi(2);
        sxy += (a - mx) * (b - my);
    }
    if sxx == 0.0 {
        return Err(Error::param("x", "every x is the same: there is no line to fit"));
    }
    let slope = sxy / sxx;
    let r = if syy == 0.0 { 0.0 } else { sxy / (sxx * syy).sqrt() };
    Ok(Fit { n, intercept: my - slope * mx, slope, r, r2: r * r })
}

/// The Moran scatter plot of `values` (one per row, all finite) for weights `rows`.
pub fn moran_scatter(values: &[f64], rows: &[Vec<(u32, f64)>]) -> Result<MoranScatter> {
    let n = values.len();
    if rows.len() != n {
        return Err(Error::param("weights", format!("the weights have {} rows and the field {n} values", rows.len())));
    }
    if n < 2 || values.iter().any(|x| !x.is_finite()) {
        return Err(Error::param("field", "needs finite values at two features or more"));
    }
    let mean = values.iter().sum::<f64>() / n as f64;
    let sd = (values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
    if sd == 0.0 {
        return Err(Error::param("field", "every value is the same: there is no variation to plot"));
    }
    let z: Vec<f64> = values.iter().map(|x| (x - mean) / sd).collect();
    let lag: Vec<f64> = rows.iter().map(|row| row.iter().map(|(j, w)| w * z.get(*j as usize).copied().unwrap_or(0.0)).sum()).collect();
    let zz: f64 = z.iter().map(|a| a * a).sum();
    let slope = z.iter().zip(&lag).map(|(a, b)| a * b).sum::<f64>() / zz;
    Ok(MoranScatter { z, lag, slope })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autocorrelation::{Statistic, global};

    #[test]
    fn histogram_bins() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        // Sturges: ⌈log₂ 100⌉ + 1 = 8 bins of width 99/8.
        let h = histogram(&v, Bins::Sturges).unwrap();
        assert_eq!(h.counts.len(), 8);
        assert_eq!(h.counts.iter().sum::<usize>(), 100);
        assert_eq!((h.edges[0], h.edges[8]), (1.0, 100.0));
        assert_eq!(h.bin_of(100.0), Some(7));
        assert_eq!(h.bin_of(1.0 + 99.0 / 8.0), Some(1), "an inner edge belongs to the bin above");
        assert_eq!(h.bin_of(0.5), None);
        // Freedman–Diaconis: IQR = 75.25 − 25.75 = 49.5, width = 99 / 100^(1/3) = 21.33…, so
        // ⌈99 / 21.33⌉ = 5 bins.
        assert_eq!(histogram(&v, Bins::FreedmanDiaconis).unwrap().counts.len(), 5);
        assert_eq!(histogram(&v, Bins::Count(4)).unwrap().counts, vec![25, 25, 25, 25]);
        assert_eq!(histogram(&[3.0, 3.0], Bins::Sturges).unwrap().counts, vec![2]);
        assert!(histogram(&[f64::NAN], Bins::Sturges).is_err());
        assert_eq!(Bins::parse("FD"), Some(Bins::FreedmanDiaconis));
        assert_eq!(Bins::parse("12"), Some(Bins::Count(12)));
        assert_eq!(Bins::parse("0"), None);
    }

    /// By hand, definition 7. For 1..=12 and 40 (13 values): Q1 at h = 3 is 4, the median 7, Q3
    /// at h = 9 is 10; IQR 6, fences −5 and 19, so 40 is the one outlier and the upper whisker
    /// is 12. For 1..=12 alone: Q1 at h = 2.75 is 3.75, the median 6.5, Q3 at h = 8.25 is 9.25.
    #[test]
    fn box_plot_by_hand() {
        let mut v: Vec<f64> = (1..=12).map(f64::from).collect();
        v.push(40.0);
        let b = box_plot(&v, 1.5).unwrap();
        assert_eq!((b.q1, b.median, b.q3, b.lower_fence, b.upper_fence), (4.0, 7.0, 10.0, -5.0, 19.0));
        assert_eq!((b.lower_whisker, b.upper_whisker), (1.0, 12.0));
        assert_eq!(b.outliers, vec![12]);
        let wide = box_plot(&v, 6.0).unwrap();
        assert!(wide.outliers.is_empty() && wide.upper_whisker == 40.0);
        let twelve: Vec<f64> = (1..=12).map(f64::from).collect();
        let b = box_plot(&twelve, 1.5).unwrap();
        assert_eq!((b.q1, b.median, b.q3), (3.75, 6.5, 9.25));
        assert!(box_plot(&v, 0.0).is_err());
    }

    #[test]
    fn least_squares_by_hand() {
        // y = 1 + 2x exactly, then with noise of known sums.
        let x = [1.0, 2.0, 3.0, 4.0];
        let f = ols(&x, &[3.0, 5.0, 7.0, 9.0]).unwrap();
        assert_eq!((f.intercept, f.slope, f.r, f.r2), (1.0, 2.0, 1.0, 1.0));
        // x̄ = 2.5, ȳ = 2.5; Sxx = 5, Syy = 5, Sxy = 4: slope 0.8, r = 0.8.
        let g = ols(&x, &[1.0, 3.0, 2.0, 4.0]).unwrap();
        assert!((g.slope - 0.8).abs() < 1e-15 && (g.r - 0.8).abs() < 1e-15 && (g.intercept - 0.5).abs() < 1e-15);
        assert!(ols(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]).is_err());
        assert!(ols(&[1.0, 2.0], &[1.0, 2.0]).is_err());
    }

    /// The Moran scatter plot's slope is the global Moran's I with row-standardised weights.
    #[test]
    fn moran_scatter_slope_is_moran_s_i() {
        let values = [8.0, 6.0, 1.0, 7.0, 3.0, 2.0];
        let adj: [&[u32]; 6] = [&[1, 3], &[0, 2, 4], &[1, 5], &[0, 4], &[1, 3, 5], &[2, 4]];
        let w: Vec<Vec<(u32, f64)>> = adj.iter().map(|r| r.iter().map(|j| (*j, 1.0 / r.len() as f64)).collect()).collect();
        let m = moran_scatter(&values, &w).unwrap();
        let i = global(Statistic::MoranI, &values, &w, 0, 0).unwrap().value;
        assert!((m.slope - i).abs() < 1e-12, "{} vs {i}", m.slope);
        assert!(m.z.iter().sum::<f64>().abs() < 1e-12);
        assert!((m.z.iter().map(|z| z * z).sum::<f64>() / 6.0 - 1.0).abs() < 1e-12);
        assert!(moran_scatter(&[1.0; 6], &w).is_err());
    }
}
