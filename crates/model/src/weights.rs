//! Spatial weights: who neighbours whom, and how much. A weights object belongs to one layer
//! and holds, for each of its features (by row), the neighbours' rows and weights, sparse.
//! `mkaegis-stats` builds them; every spatial statistic takes one.

use mkaegis_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// How the neighbours were found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Method {
    /// Polygons sharing a vertex (`queen`) or an edge (rook), up to `order` steps away;
    /// `cumulative` keeps the lower orders too.
    Contiguity { queen: bool, order: u32, cumulative: bool },
    /// The `k` nearest features.
    Nearest { k: u32 },
    /// Every feature within `threshold` metres; weights 1/d^`power` when `power` is set.
    DistanceBand { threshold: f64, power: Option<f64> },
    /// A kernel of the distance over the bandwidth: fixed (metres) or adaptive (the distance to
    /// each feature's `k`-th neighbour).
    Kernel { kernel: Kernel, bandwidth: Bandwidth, diagonal: bool },
    /// Read from a GAL or GWT file.
    File { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kernel {
    Uniform,
    Triangular,
    Bisquare,
    Gaussian,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Bandwidth {
    Fixed(f64),
    Adaptive(u32),
}

/// How the weights are scaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Standardization {
    /// Each row sums to 1 (W).
    Row,
    /// 1 for a neighbour, 0 otherwise (B).
    Binary,
    /// Variance-stabilising (S, Tiefelsdorf, Griffith & Boots 1999): rows scaled by their
    /// Euclidean norm, then all weights so that they sum to n.
    Variance,
    /// As found (the kernel or inverse-distance values; 1 for contiguity and nearest neighbours).
    None,
}

/// A weights matrix for one layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weights {
    pub name: String,
    /// The layer it was built from.
    pub layer: u32,
    pub method: Method,
    pub standardization: Standardization,
    /// For each row of the layer, its neighbours: (row, weight), in row order.
    pub neighbours: Vec<Vec<(u32, f64)>>,
}

/// What a statistician checks before trusting a weights matrix.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub n: usize,
    /// Non-zero weights.
    pub links: usize,
    pub percent_nonzero: f64,
    pub mean_neighbours: f64,
    pub min_neighbours: usize,
    pub max_neighbours: usize,
    /// Rows with no neighbour.
    pub islands: Vec<usize>,
    /// Whether w_ij ≠ 0 exactly when w_ji ≠ 0.
    pub symmetric: bool,
    /// (number of neighbours, how many rows have it), ascending.
    pub histogram: Vec<(usize, usize)>,
    /// S₀, the sum of all weights.
    pub s0: f64,
}

impl Standardization {
    pub fn parse(text: &str) -> Option<Standardization> {
        match text.trim().to_ascii_lowercase().as_str() {
            "row" | "r" | "w" => Some(Standardization::Row),
            "binary" | "b" => Some(Standardization::Binary),
            "variance" | "s" => Some(Standardization::Variance),
            "none" | "raw" | "c" => Some(Standardization::None),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Standardization::Row => "row",
            Standardization::Binary => "binary",
            Standardization::Variance => "variance",
            Standardization::None => "none",
        }
    }
}

impl Weights {
    /// The number of rows the weights cover.
    pub fn n(&self) -> usize {
        self.neighbours.len()
    }

    /// Fails unless the weights still fit `layer` with `rows` features (they go stale when
    /// features are added or deleted).
    pub fn check_fits(&self, layer: u32, rows: usize) -> Result<()> {
        if self.layer != layer {
            return Err(Error::param("weights", format!("`{}` was built for another layer", self.name)));
        }
        if self.n() != rows {
            return Err(Error::param(
                "weights",
                format!("`{}` was built for {} features and the layer has {rows}: build the weights again", self.name, self.n()),
            ));
        }
        Ok(())
    }

    /// The neighbours of `row`, empty for an unknown row.
    pub fn of(&self, row: usize) -> &[(u32, f64)] {
        self.neighbours.get(row).map_or(&[], Vec::as_slice)
    }

    pub fn summary(&self) -> Summary {
        let n = self.n();
        let counts: Vec<usize> = self.neighbours.iter().map(|row| row.iter().filter(|(_, w)| *w != 0.0).count()).collect();
        let links: usize = counts.iter().sum();
        let mut histogram = std::collections::BTreeMap::new();
        for c in &counts {
            *histogram.entry(*c).or_insert(0usize) += 1;
        }
        let symmetric = self.neighbours.iter().enumerate().all(|(i, row)| {
            row.iter().filter(|(_, w)| *w != 0.0).all(|(j, _)| self.of(*j as usize).iter().any(|(k, w)| *k as usize == i && *w != 0.0))
        });
        Summary {
            n,
            links,
            percent_nonzero: if n == 0 { 0.0 } else { 100.0 * links as f64 / (n as f64 * n as f64) },
            mean_neighbours: if n == 0 { 0.0 } else { links as f64 / n as f64 },
            min_neighbours: counts.iter().copied().min().unwrap_or(0),
            max_neighbours: counts.iter().copied().max().unwrap_or(0),
            islands: counts.iter().enumerate().filter(|(_, c)| **c == 0).map(|(i, _)| i).collect(),
            symmetric,
            histogram: histogram.into_iter().collect(),
            s0: self.neighbours.iter().flatten().map(|(_, w)| w).sum(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_links_islands_and_symmetry() {
        let w = Weights {
            name: "w".into(),
            layer: 1,
            method: Method::Nearest { k: 1 },
            standardization: Standardization::None,
            neighbours: vec![vec![(1, 1.0)], vec![(0, 1.0)], vec![(1, 1.0)], vec![]],
        };
        let s = w.summary();
        assert_eq!((s.n, s.links, s.min_neighbours, s.max_neighbours), (4, 3, 0, 1));
        assert_eq!(s.islands, vec![3]);
        assert!(!s.symmetric, "2 → 1 has no 1 → 2");
        assert_eq!(s.histogram, vec![(0, 1), (1, 3)]);
        assert!((s.percent_nonzero - 18.75).abs() < 1e-12);
        assert_eq!(s.s0, 3.0);
        assert!(w.check_fits(1, 4).is_ok());
        assert!(w.check_fits(1, 5).is_err());
        assert!(w.check_fits(2, 4).is_err());
        assert!(w.of(99).is_empty());
    }
}
