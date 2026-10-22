//! What the tests share: the standard normal, χ² and Student's t tails for p-values, and a seeded
//! random number generator for permutation tests, so a result can be reproduced from its seed.
//!
//! The χ² and t tails come from the regularised incomplete gamma and beta functions, evaluated by
//! their series and continued fractions (Abramowitz and Stegun 1964, 6.5.29, 6.5.31 and
//! 26.5.8), with ln Γ by Lanczos's approximation (g = 7, nine coefficients).

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

/// P(χ²_df ≥ x): the upper tail of the chi-squared distribution with `df` degrees of freedom.
pub fn chi2_sf(x: f64, df: f64) -> f64 {
    if x.is_nan() || df.is_nan() || df <= 0.0 {
        return f64::NAN;
    }
    if x <= 0.0 {
        return 1.0;
    }
    gamma_q(df / 2.0, x / 2.0)
}

/// The two-sided p-value of Student's t with `df` degrees of freedom: P(|T| ≥ |t|).
pub fn t_two_sided_p(t: f64, df: f64) -> f64 {
    if t.is_nan() || df.is_nan() || df <= 0.0 {
        return f64::NAN;
    }
    if t.is_infinite() {
        return 0.0;
    }
    // P(|T| ≥ |t|) = I_x(df/2, 1/2) with x = df / (df + t²).
    beta_reg(df / (df + t * t), df / 2.0, 0.5).clamp(0.0, 1.0)
}

/// ln Γ(x) for x > 0, by Lanczos's approximation (g = 7, n = 9; about 15 digits).
fn ln_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection: Γ(x)Γ(1 − x) = π / sin(πx).
        return (PI / (PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + G + 0.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    0.5 * (2.0 * PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// The regularised upper incomplete gamma function Q(a, x) = Γ(a, x) / Γ(a).
fn gamma_q(a: f64, x: f64) -> f64 {
    let front = (a * x.ln() - x - ln_gamma(a)).exp();
    if x < a + 1.0 {
        // Series for P(a, x) = e^(−x) x^a / Γ(a) · Σ x^n / (a (a+1) … (a+n)).
        let (mut term, mut sum, mut ap) = (1.0 / a, 1.0 / a, a);
        for _ in 0..10_000 {
            ap += 1.0;
            term *= x / ap;
            sum += term;
            if term.abs() < sum.abs() * 1e-16 {
                break;
            }
        }
        (1.0 - front * sum).clamp(0.0, 1.0)
    } else {
        // Continued fraction for Q(a, x), by Lentz's method.
        let tiny = 1e-300;
        let mut b = x + 1.0 - a;
        let (mut c, mut d) = (1.0 / tiny, 1.0 / b);
        let mut h = d;
        for i in 1..10_000 {
            let an = -f64::from(i) * (f64::from(i) - a);
            b += 2.0;
            d = an * d + b;
            d = if d.abs() < tiny { 1.0 / tiny } else { 1.0 / d };
            c = b + an / c;
            if c.abs() < tiny {
                c = tiny;
            }
            let delta = d * c;
            h *= delta;
            if (delta - 1.0).abs() < 1e-16 {
                break;
            }
        }
        (front * h).clamp(0.0, 1.0)
    }
}

/// The regularised incomplete beta function I_x(a, b).
fn beta_reg(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    // The continued fraction converges fast for x < (a + 1) / (a + b + 2); else use symmetry.
    if x < (a + 1.0) / (a + b + 2.0) { front * beta_cf(x, a, b) / a } else { 1.0 - front * beta_cf(1.0 - x, b, a) / b }
}

/// The continued fraction of the incomplete beta function (Abramowitz and Stegun 26.5.8), Lentz.
fn beta_cf(x: f64, a: f64, b: f64) -> f64 {
    let tiny = 1e-300;
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    d = if d.abs() < tiny { 1.0 / tiny } else { 1.0 / d };
    let mut h = d;
    for m in 1..10_000 {
        let m = f64::from(m);
        let m2 = 2.0 * m;
        for aa in [m * (b - m) * x / ((qam + m2) * (a + m2)), -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2))] {
            d = 1.0 + aa * d;
            d = if d.abs() < tiny { 1.0 / tiny } else { 1.0 / d };
            c = 1.0 + aa / c;
            if c.abs() < tiny {
                c = tiny;
            }
            h *= d * c;
        }
        if (d * c - 1.0).abs() < 1e-16 {
            break;
        }
    }
    h
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

    /// Closed forms: χ²₂ tail = e^(−x/2); χ²₁ tail = erfc(√(x/2)); t₁ (Cauchy) two-sided
    /// p = 1 − 2·atan|t|/π; t₂ two-sided p = 1 − |t|/√(2 + t²).
    #[test]
    fn chi_squared_and_t_tails() {
        for x in [0.1, 1.0, 3.84, 10.0, 50.0] {
            assert!((chi2_sf(x, 2.0) - (-x / 2.0).exp()).abs() < 1e-13, "χ²₂ {x}");
            assert!((chi2_sf(x, 1.0) - erfc((x / 2.0).sqrt())).abs() < 1e-12, "χ²₁ {x}");
        }
        for t in [0.2f64, 1.0, 2.5, 12.0] {
            let cauchy = 1.0 - 2.0 * t.atan() / PI;
            assert!((t_two_sided_p(t, 1.0) - cauchy).abs() < 1e-12, "t₁ {t}");
            assert!((t_two_sided_p(-t, 2.0) - (1.0 - t / (2.0 + t * t).sqrt())).abs() < 1e-12, "t₂ {t}");
        }
        // Large df: t tends to the normal.
        assert!((t_two_sided_p(1.959_963_984_540_054, 1e7) - 0.05).abs() < 1e-6);
        assert_eq!(chi2_sf(0.0, 3.0), 1.0);
        assert!(chi2_sf(1.0, 0.0).is_nan() && t_two_sided_p(1.0, -1.0).is_nan());
        // Γ(5) = 24, Γ(0.5) = √π.
        assert!((ln_gamma(5.0) - 24f64.ln()).abs() < 1e-13 && (ln_gamma(0.5) - PI.sqrt().ln()).abs() < 1e-13);
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
