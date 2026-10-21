//! A static k-d tree over 3-D points, for nearest-neighbour and radius queries. Built and
//! searched with explicit stacks: no recursion, whatever the input.

/// Leaves hold at most this many points.
const LEAF: usize = 8;

pub struct KdTree {
    points: Vec<[f64; 3]>,
    /// Point indices, reordered so that every node is a contiguous range.
    order: Vec<usize>,
    nodes: Vec<Node>,
}

struct Node {
    start: usize,
    end: usize,
    /// (axis, split value, left child, right child) for an inner node.
    split: Option<(usize, f64, usize, usize)>,
}

impl KdTree {
    pub fn new(points: Vec<[f64; 3]>) -> Self {
        let mut order: Vec<usize> = (0..points.len()).collect();
        let mut nodes = vec![Node { start: 0, end: points.len(), split: None }];
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            let Some(node) = nodes.get(id) else { continue };
            let (start, end) = (node.start, node.end);
            if end - start <= LEAF {
                continue;
            }
            let Some(slice) = order.get_mut(start..end) else { continue };
            let axis = widest_axis(&points, slice);
            let mid = slice.len() / 2;
            let coord = |i: &usize| points.get(*i).map_or(0.0, |p| p[axis]);
            slice.select_nth_unstable_by(mid, |a, b| coord(a).total_cmp(&coord(b)));
            let value = slice.get(mid).map_or(0.0, coord);
            let (left, right) = (nodes.len(), nodes.len() + 1);
            nodes.push(Node { start, end: start + mid, split: None });
            nodes.push(Node { start: start + mid, end, split: None });
            if let Some(node) = nodes.get_mut(id) {
                node.split = Some((axis, value, left, right));
            }
            stack.extend([left, right]);
        }
        KdTree { points, order, nodes }
    }

    /// The `k` points nearest to `q` (squared distance, index), nearest first, ties by index.
    pub fn nearest(&self, q: [f64; 3], k: usize) -> Vec<(f64, usize)> {
        let mut best: Vec<(f64, usize)> = Vec::with_capacity(k + 1);
        if k == 0 {
            return best;
        }
        let mut stack = vec![(0usize, 0.0f64)];
        while let Some((id, bound)) = stack.pop() {
            if best.len() == k && best.last().is_some_and(|w| bound > w.0) {
                continue;
            }
            let Some(node) = self.nodes.get(id) else { continue };
            match node.split {
                None => {
                    for &i in self.order.get(node.start..node.end).unwrap_or(&[]) {
                        let d = self.points.get(i).map_or(f64::INFINITY, |p| squared(*p, q));
                        let at = best.partition_point(|&(bd, bi)| bd < d || (bd == d && bi < i));
                        if at < k {
                            best.insert(at, (d, i));
                            best.truncate(k);
                        }
                    }
                }
                Some((axis, value, left, right)) => {
                    let diff = q[axis] - value;
                    let (near, far) = if diff < 0.0 { (left, right) } else { (right, left) };
                    stack.push((far, bound.max(diff * diff)));
                    stack.push((near, bound));
                }
            }
        }
        best
    }

    /// Every point within `radius` of `q` (squared distance, index), in index order.
    pub fn within(&self, q: [f64; 3], radius: f64) -> Vec<(f64, usize)> {
        let r2 = radius * radius;
        let mut out = Vec::new();
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            let Some(node) = self.nodes.get(id) else { continue };
            match node.split {
                None => {
                    for &i in self.order.get(node.start..node.end).unwrap_or(&[]) {
                        let d = self.points.get(i).map_or(f64::INFINITY, |p| squared(*p, q));
                        if d <= r2 {
                            out.push((d, i));
                        }
                    }
                }
                Some((axis, value, left, right)) => {
                    let diff = q[axis] - value;
                    if diff - radius <= 0.0 {
                        stack.push(left);
                    }
                    if diff + radius >= 0.0 {
                        stack.push(right);
                    }
                }
            }
        }
        out.sort_by_key(|&(_, i)| i);
        out
    }
}

fn widest_axis(points: &[[f64; 3]], idx: &[usize]) -> usize {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in idx.iter().filter_map(|i| points.get(*i)) {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap_or(0)
}

fn squared(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_match_brute_force() {
        // A deterministic scatter, with duplicates.
        let mut seed = 12_345u64;
        let mut next = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut pts: Vec<[f64; 3]> = (0..500).map(|_| [next(), next(), 0.0]).collect();
        pts.extend_from_slice(&pts.clone()[..20]);
        let tree = KdTree::new(pts.clone());
        for q in pts.iter().take(40) {
            let mut brute: Vec<(f64, usize)> = pts.iter().enumerate().map(|(i, p)| (squared(*p, *q), i)).collect();
            brute.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            brute.truncate(7);
            assert_eq!(tree.nearest(*q, 7), brute);
            let mut inside: Vec<(f64, usize)> = pts.iter().enumerate().map(|(i, p)| (squared(*p, *q), i)).filter(|(d, _)| *d <= 0.01).collect();
            inside.sort_by_key(|&(_, i)| i);
            assert_eq!(tree.within(*q, 0.1), inside);
        }
    }

    #[test]
    fn degenerate_inputs() {
        let empty = KdTree::new(vec![]);
        assert!(empty.nearest([0.0; 3], 3).is_empty());
        assert!(empty.within([0.0; 3], 1.0).is_empty());
        let same = KdTree::new(vec![[1.0, 1.0, 1.0]; 100]);
        assert_eq!(same.nearest([1.0; 3], 3), vec![(0.0, 0), (0.0, 1), (0.0, 2)]);
        assert_eq!(same.within([1.0; 3], 0.0).len(), 100);
    }
}
