//! What the tests share: the standard normal's tail for p-values, and a seeded random number
//! generator for permutation tests, so a result can be reproduced from its seed.

use std::f64::consts::PI;

/// The two-sided p-value of a standard normal z: P(|Z| ≥ |z|). NaN for a non-finite z.
pub fn two_sided_p(z: f64) -> f64 {
    if z.is_nan() {
        return f64::NAN;
    }
    if z.is_infinite() {
        return 0.0;
    }
    erfc(z.abs() / std::f64::consts::SQRT_2).min(1.0)
}

/// The complementary error function for x ≥ 0, to about 1e-14 relative: below 1.5, one minus
/// the series erf(x) = 2/√π · e^(−x²) · Σ 2ⁿ x^(2n+1) / (1·3·…·(2n+1)) (all terms positive, so
/// no cancellation); from 1.5, the continued fraction
/// erfc(x) = e^(−x²)/√π · 1/(x + (1/2)/(x + 1/(x + (3/2)/(x + …)))), evaluated by Lentz's method.
fn erfc(x: f64) -> f64 {
    if x < 1.5 {
        let (mut term, mut sum) = (x, x);
        for n in 1..200 {
            term *= 2.0 * x * x / f64::from(2 * n + 1);
            sum += term;
            if term < sum * 1e-17 {
                break;
            }
        }
        return 1.0 - 2.0 / PI.sqrt() * (-x * x).exp() * sum;
    }
    // Lentz: f = x + a1/(x + a2/(x + …)), a_k = k/2.
    let tiny = 1e-300;
    let (mut f, mut c, mut d) = (x, x, 0.0);
    for k in 1..5000 {
        let a = f64::from(k) / 2.0;
        d = x + a * d;
        d = if d.abs() < tiny { 1.0 / tiny } else { 1.0 / d };
        c = x + a / c;
        if c.abs() < tiny {
            c = tiny;
        }
        let delta = c * d;
        f *= delta;
        if (delta - 1.0).abs() < 1e-16 {
            break;
        }
    }
    (-x * x).exp() / PI.sqrt() / f
}

/// SplitMix64 (Steele, Lea and Flood 2014): small, fast and fully determined by its seed.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A uniform index below `n` (n > 0), without modulo bias (rejection).
    pub fn below(&mut self, n: usize) -> usize {
        let n = n.max(1) as u64;
        let zone = u64::MAX - u64::MAX % n;
        loop {
            let x = self.next_u64();
            if x < zone {
                return (x % n) as usize;
            }
        }
    }

    /// Shuffles `items` in place (Fisher–Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference values from the C library's erfc (Python's `math.erfc`), re-typed.
    #[test]
    fn normal_tails() {
        for (x, expected) in [
            (0.0, 1.0),
            (0.5, 0.479_500_122_186_953_5),
            (1.0, 0.157_299_207_050_285_13),
            (2.0, 0.004_677_734_981_047_266),
            (1.4999, 0.033_906_748_337_704_73),
            (1.5, 0.033_894_853_524_689_274),
            (2.4999, 0.000_407_169_900_333_451_4),
            (2.5, 0.000_406_952_017_444_958_9),
            (3.0, 2.209_049_699_858_544e-5),
            (6.0, 2.151_973_671_249_891_3e-17),
            (20.0, 5.395_865_611_607_901e-176),
        ] {
            let got = erfc(x);
            assert!((got - expected).abs() <= expected * 1e-13, "erfc({x}) = {got}, not {expected}");
        }
        assert!((two_sided_p(1.959_963_984_540_054) - 0.05).abs() < 1e-14);
        assert_eq!(two_sided_p(-1.0), two_sided_p(1.0));
        assert_eq!(two_sided_p(f64::INFINITY), 0.0);
        assert!(two_sided_p(f64::NAN).is_nan());
    }

    #[test]
    fn the_generator_is_reproducible_and_uniform() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        assert!((0..100).all(|_| a.next_u64() == b.next_u64()));
        let mut counts = [0u32; 10];
        let mut r = Rng::new(1);
        for _ in 0..100_000 {
            counts[r.below(10)] += 1;
        }
        assert!(counts.iter().all(|c| (9_500..10_500).contains(c)), "{counts:?}");
        let mut v: Vec<u32> = (0..50).collect();
        r.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(v, sorted);
    }
}
