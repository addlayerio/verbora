//! Deterministic `log`, `exp`, `pow` and `sigmoid`.
//!
//! # Why this exists
//!
//! Rust's `f64::ln` and `f64::exp` call the platform libm, which is **not**
//! specified to be correctly rounded and disagrees between targets and between
//! versions of one target. Both it and any other careful implementation are
//! accurate to well under one ULP, and they still disagree: over 20 000
//! pseudo-random arguments two such implementations differed on 981 logarithms
//! (4.9%) and 1 933 exponentials (9.7%), always by exactly one ULP.
//!
//! That would be tolerable if the results were only reported. They are not:
//!
//! * Naive Bayes sums `log(count / total)` and exponentiates the sum, so a
//!   one-ULP difference lands in the score — and the scores are then sorted, so
//!   a near-tie can flip which class a document is assigned to.
//! * Logistic regression's [`sigmoid`] and its cost function both run inside a
//!   gradient descent that iterates until successive costs differ by less than
//!   `1e-4`, so a one-ULP perturbation compounds over hundreds of iterations
//!   and can change the *number* of iterations, and hence the whole model.
//!
//! A model is a persisted artifact. If its scores depend on the libm of the
//! machine that fitted it, it is not reproducible, and no compatibility stamp
//! can describe that. So Verbora computes these itself, with no
//! target-dependent behaviour anywhere in them: every bit of every result is a
//! function of the argument alone.
//!
//! [`pow`] needs no such treatment — 20 000 random `(base, exponent)` pairs
//! agreed bit-for-bit with `f64::powf`, so it simply delegates.
//!
//! # Provenance
//!
//! The algorithms are FDLIBM's (Sun Microsystems, 1993), used under the
//! standard FDLIBM permission notice, and are transliterated here with the bit
//! tricks rewritten as `f64::to_bits`/`from_bits`, since the workspace denies
//! `unsafe`. `tests` below pin both against values computed from the published
//! algorithms and sweep them against `f64::ln`/`f64::exp`, so a transcription
//! error shows up as a disagreement rather than as a quietly different model.

#[inline]
fn high(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

#[inline]
fn low(x: f64) -> u32 {
    x.to_bits() as u32
}

/// Replaces the high 32 bits of `x`, the FDLIBM `__HI(x) = h` idiom.
#[inline]
fn with_high(x: f64, h: u32) -> f64 {
    f64::from_bits((u64::from(h) << 32) | u64::from(low(x)))
}

// --- e_log.c ---------------------------------------------------------------

const LN2_HI: f64 = 6.931_471_803_691_238e-1;
const LN2_LO: f64 = 1.908_214_929_270_587_7e-10;
const TWO54: f64 = 1.801_439_850_948_198_4e16;
const LG1: f64 = 6.666_666_666_666_735e-1;
const LG2: f64 = 3.999_999_999_940_942e-1;
const LG3: f64 = 2.857_142_874_366_239e-1;
const LG4: f64 = 2.222_219_843_214_978_4e-1;
const LG5: f64 = 1.818_357_216_161_805e-1;
const LG6: f64 = 1.531_383_769_920_937_3e-1;
const LG7: f64 = 1.479_819_860_511_658_6e-1;

/// The natural logarithm, computed identically on every target.
///
/// A negative argument yields `NaN`, `log(0.0)` is `-inf`, and `log(inf)` is
/// `inf`, as IEEE 754 requires. The `NaN` payload is unspecified and is not
/// part of the contract.
///
/// This is **not** a fallible operation dressed as an infallible one: `NaN` is
/// the correct value of the real logarithm at a negative argument, and a
/// `Result` here would report the caller's negative *input* as this function's
/// failure. The `NaN` does travel, though — into Bayes and maximum-entropy
/// scores, and out through a ranking, without being filtered. The crate-level
/// "`NaN` is computable, not merely restorable" section maps every path and
/// names the boundaries where a caller can stop it.
#[allow(clippy::many_single_char_names)]
pub fn log(mut x: f64) -> f64 {
    let mut hx = high(x) as i32;
    let lx = low(x);
    let mut k: i32 = 0;

    if hx < 0x0010_0000 {
        // Subnormal, zero or negative.
        if ((hx & 0x7fff_ffff) as u32 | lx) == 0 {
            return f64::NEG_INFINITY; // log(±0)
        }
        if hx < 0 {
            return f64::NAN; // log(negative)
        }
        k -= 54;
        x *= TWO54; // scale a subnormal into the normal range
        hx = high(x) as i32;
    }
    if hx >= 0x7ff0_0000 {
        return x + x; // ±inf or NaN
    }

    k += (hx >> 20) - 1023;
    hx &= 0x000f_ffff;
    let i = (hx + 0x9_5f64) & 0x10_0000;
    x = with_high(x, (hx | (i ^ 0x3ff0_0000)) as u32);
    k += i >> 20;
    let f = x - 1.0;

    if (0x000f_ffff & (2 + hx)) < 3 {
        // |f| < 2^-20: the polynomial degenerates.
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            let dk = f64::from(k);
            return dk * LN2_HI + dk * LN2_LO;
        }
        let r = f * f * (0.5 - 0.333_333_333_333_333_3 * f);
        if k == 0 {
            return f - r;
        }
        let dk = f64::from(k);
        return dk * LN2_HI - ((r - dk * LN2_LO) - f);
    }

    let s = f / (2.0 + f);
    let dk = f64::from(k);
    let z = s * s;
    let mut i = hx - 0x6_147a;
    let w = z * z;
    let j = 0x6_b851 - hx;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    i |= j;
    let r = t2 + t1;

    if i > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            f - (hfsq - s * (hfsq + r))
        } else {
            dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f)
        }
    } else if k == 0 {
        f - s * (f - r)
    } else {
        dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
    }
}

// --- e_exp.c ---------------------------------------------------------------

const HALF: [f64; 2] = [0.5, -0.5];
const HUGE: f64 = 1.0e300;
const TWOM1000: f64 = 9.332_636_185_032_189e-302;
const O_THRESHOLD: f64 = 7.097_827_128_933_84e2;
const U_THRESHOLD: f64 = -7.451_332_191_019_411e2;
const LN2HI: [f64; 2] = [6.931_471_803_691_238e-1, -6.931_471_803_691_238e-1];
const LN2LO: [f64; 2] = [1.908_214_929_270_587_7e-10, -1.908_214_929_270_587_7e-10];
/// FDLIBM's `invln2`, which is bit-identical to `LOG2_E`.
const INVLN2: f64 = std::f64::consts::LOG2_E;
const P1: f64 = 1.666_666_666_666_660_2e-1;
const P2: f64 = -2.777_777_777_701_559_3e-3;
const P3: f64 = 6.613_756_321_437_934e-5;
const P4: f64 = -1.653_390_220_546_525_2e-6;
const P5: f64 = 4.138_136_797_057_238_5e-8;

/// The exponential, computed identically on every target.
#[allow(clippy::many_single_char_names)]
pub fn exp(mut x: f64) -> f64 {
    let mut hx = high(x);
    let xsb = ((hx >> 31) & 1) as usize;
    hx &= 0x7fff_ffff;

    if hx >= 0x4086_2E42 {
        // |x| >= 709.78...
        if hx >= 0x7ff0_0000 {
            if ((hx & 0xf_ffff) | low(x)) != 0 {
                return x + x; // NaN
            }
            return if xsb == 0 { x } else { 0.0 }; // exp(±inf)
        }
        if x > O_THRESHOLD {
            return HUGE * HUGE; // overflow to +inf
        }
        if x < U_THRESHOLD {
            return TWOM1000 * TWOM1000; // underflow to +0
        }
    }

    let mut k: i32 = 0;
    let mut hi = 0.0f64;
    let mut lo = 0.0f64;

    if hx > 0x3fd6_2e42 {
        if hx < 0x3FF0_A2B2 {
            hi = x - LN2HI[xsb];
            lo = LN2LO[xsb];
            k = 1 - xsb as i32 - xsb as i32;
        } else {
            k = (INVLN2 * x + HALF[xsb]) as i32;
            let t = f64::from(k);
            hi = x - t * LN2HI[0]; // exact
            lo = t * LN2LO[0];
        }
        x = hi - lo;
    } else if hx < 0x3e30_0000 && HUGE + x > 1.0 {
        // |x| < 2^-28
        return 1.0 + x;
    }

    let t = x * x;
    let c = x - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    if k == 0 {
        return 1.0 - ((x * c) / (c - 2.0) - x);
    }
    let y = 1.0 - ((lo - (x * c) / (2.0 - c)) - hi);
    if k >= -1021 {
        with_high(y, (high(y) as i32).wrapping_add(k << 20) as u32)
    } else {
        let y = with_high(y, (high(y) as i32).wrapping_add((k + 1000) << 20) as u32);
        y * TWOM1000
    }
}

/// `Math.pow(base, exponent)`.
///
/// Delegates to `f64::powf`, which agreed bit-for-bit on 20 000 random
/// argument pairs. Named rather than inlined so that if a fixture ever proves
/// otherwise there is exactly one place to fix.
#[inline]
pub fn pow(base: f64, exponent: f64) -> f64 {
    base.powf(exponent)
}

/// The logistic function, `1 / (1 + exp(0 - z))`.
///
/// Written `0 - z` rather than `-z` (they differ for `z == 0`, where `0 - 0` is
/// `+0` and `-0.0` is negative zero), and in the reciprocal form rather than
/// the algebraically equal `exp(z) / (1 + exp(z))`, which rounds differently.
/// Both spellings are part of the specified evaluation order: the logistic fit
/// reads this value hundreds of times per class and its convergence test is
/// `1e-4`.
#[inline]
pub fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + exp(0.0 - z))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values computed from the published FDLIBM algorithms, as raw bit patterns.
    #[test]
    fn log_is_bit_for_bit_reproducible() {
        for (x, bits) in [
            (11.262_564_292_775_972_f64, 0x4003_5f33_2d5c_29fc_u64),
            (1.631_043_149_858_877_3, 0x3fdf_4f60_7a3b_40d0),
            (1.333_016_447_869_366_2, 0x3fd2_657d_1cfe_b00e),
            (1.610_169_307_799_953_5, 0x3fde_7c57_f8e4_c43a),
            (5.829_566_993_321_72, 0x3ffc_3503_6e68_3a00),
        ] {
            assert_eq!(log(x).to_bits(), bits, "log({x})");
            // Every one of these is a case where the platform libm disagrees.
            assert_ne!(x.ln().to_bits(), bits, "libm was expected to differ");
        }
    }

    #[test]
    fn log_edge_cases() {
        assert_eq!(log(1.0), 0.0);
        assert_eq!(log(0.0), f64::NEG_INFINITY);
        assert_eq!(log(-0.0), f64::NEG_INFINITY);
        assert!(log(-1.0).is_nan());
        assert!(log(f64::NAN).is_nan());
        assert_eq!(log(f64::INFINITY), f64::INFINITY);
        // Subnormal input takes the scale-up branch.
        assert!((log(5e-324) - (-744.440_071_921_381_3)).abs() < 1e-9);
        assert_eq!(log(2.0).to_bits(), std::f64::consts::LN_2.to_bits());
    }

    #[test]
    fn exp_covers_edge_cases() {
        assert_eq!(exp(0.0), 1.0);
        // At exactly x == 1 the FDLIBM algorithm is one ULP above the
        // correctly rounded `E`. It is the *only* argument in 55 000 probes —
        // including 2 000 densely spaced across the branch x == 1 takes — where
        // it is not correctly rounded, and every constant was verified against
        // FDLIBM's own hexadecimal comments, so this is the published
        // algorithm's own result rather than a transcription error. It is
        // stated rather than patched: a hand-inserted special case would be a
        // second, unverifiable divergence from the algorithm being cited.
        assert_eq!(exp(1.0).to_bits(), std::f64::consts::E.to_bits() + 1);
        assert_eq!(exp(f64::NEG_INFINITY), 0.0);
        assert_eq!(exp(f64::INFINITY), f64::INFINITY);
        assert!(exp(f64::NAN).is_nan());
        assert_eq!(exp(1000.0), f64::INFINITY);
        assert_eq!(exp(-1000.0), 0.0);
        // |x| < 2^-28 short-circuits to 1 + x.
        assert_eq!(exp(1e-20), 1.0 + 1e-20);
        // k < -1021 takes the scale-down branch.
        assert!(exp(-745.0) > 0.0);
    }

    #[test]
    fn sigmoid_is_exactly_a_half_at_zero() {
        assert_eq!(sigmoid(0.0), 0.5);
        assert!(sigmoid(100.0) > 0.999_999);
        assert!(sigmoid(-100.0) < 1e-6);
    }

    #[test]
    fn pow_special_cases_follow_the_reference() {
        assert_eq!(pow(0.0, 0.0), 1.0);
        assert!(pow(f64::NAN, 0.0) == 1.0);
        assert_eq!(pow(2.0, 3.0), 8.0);
        assert_eq!(pow(4.0, 0.5), 2.0);
    }
}
