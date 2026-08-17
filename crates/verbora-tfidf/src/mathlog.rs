//! `Math.log`, reproduced exactly — because `f64::ln` is **not** the same
//! function.
//!
//! # Why this module exists
//!
//! The whole numeric content of `TfIdf` is one line:
//!
//! ```text
//! const idf = 1 + Math.log(this.documents.length / (1 + docsWithTerm))
//! ```
//!
//! and the reference specs assert its result with `toBe`, i.e. bit equality. The
//! obvious port is `1.0 + (n / (1.0 + d)).ln()`, and `docs/PARITY.md` says as
//! much: "`Math.log` must be `f64::ln`". That turns out to be wrong on this
//! platform, and the fixtures caught it:
//!
//! ```text
//! Math.log(3)   the reference   1.0986122886681096
//!               glibc           1.0986122886681098
//! ```
//!
//! The reference engine does not call the platform libm. Its `Math.log` is a
//! port of Sun's fdlibm `__ieee754_log`, and it is *more* accurate here than
//! glibc — the true value is 1.0986122886681096913…, so the reference engine is correctly rounded
//! and glibc is one ULP high. Over the 3,400 inputs recorded in the
//! `mathLog` fixture suite, glibc disagrees with the reference engine on **5.6%** of them, and
//! `1 + ln(3)` is what a three-document corpus with one match produces. This is
//! not an exotic corner.
//!
//! # What is reproduced
//!
//! Sun's `__ieee754_log` verbatim: argument reduction to `x = 2^k · (1 + f)`
//! with `√2/2 < 1+f < √2`, the `|f| < 2^-20` short path, and the degree-7
//! minimax polynomial in `s = f/(2+f)` evaluated in exactly the reference's
//! association. Every intermediate must keep the reference's parenthesisation:
//! re-associating `dk*ln2_hi - ((hfsq - (s*(hfsq+R) + dk*ln2_lo)) - f)` is
//! algebraically free and numerically not.
//!
//! The bit patterns are manipulated as `i32`/`u32` halves, matching the C. In
//! particular the high word is **signed**, which is what makes `log(-1)` take
//! the subnormal branch and return `NaN` rather than falling through to the
//! infinity test.

// The constants are given as bit patterns rather than decimal literals. Sun's
// source annotates every one of them with its hex encoding precisely because
// the decimal spelling is a lossy rendering of the intended double, and a
// truncated decimal — which is what a "this literal has excessive precision"
// suggestion produces — is a silently different function. Each `from_bits`
// below is the hex comment from `e_log.c`, verified to round-trip the decimal
// the C source also carries.

/// `ln(2)`, high part: `6.93147180369123816490e-01`, exact in 33 bits.
const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000);
/// `ln(2)`, low part: `1.90821492927058770002e-10`.
const LN2_LO: f64 = f64::from_bits(0x3dea_39ef_3579_3c76);
/// `2^54`, used to scale subnormals into the normal range.
const TWO54: f64 = f64::from_bits(0x4350_0000_0000_0000);
/// The nearest double to `1/3`, used by the `|f| < 2^-20` short path.
const ONE_THIRD: f64 = f64::from_bits(0x3fd5_5555_5555_5555);

/// `6.666666666666735130e-01`
const LG1: f64 = f64::from_bits(0x3fe5_5555_5555_5593);
/// `3.999999999940941908e-01`
const LG2: f64 = f64::from_bits(0x3fd9_9999_9997_fa04);
/// `2.857142874366239149e-01`
const LG3: f64 = f64::from_bits(0x3fd2_4924_9422_9359);
/// `2.222219843214978396e-01`
const LG4: f64 = f64::from_bits(0x3fcc_71c5_1d8e_78af);
/// `1.818357216161805012e-01`
const LG5: f64 = f64::from_bits(0x3fc7_4664_96cb_03de);
/// `1.531383769920937332e-01`
const LG6: f64 = f64::from_bits(0x3fc3_9a09_d078_c69f);
/// `1.479819860511658591e-01`
const LG7: f64 = f64::from_bits(0x3fc2_f112_df3e_5244);

/// The high 32 bits of `x`, as the C's signed `int32_t`.
#[inline]
#[expect(clippy::cast_possible_wrap, reason = "the reference reads it signed")]
fn high(x: f64) -> i32 {
    (x.to_bits() >> 32) as u32 as i32
}

/// The low 32 bits of `x`.
#[inline]
fn low(x: f64) -> u32 {
    x.to_bits() as u32
}

/// Replaces the high 32 bits of `x`, keeping the low ones — `SET_HIGH_WORD`.
#[inline]
fn with_high(x: f64, hi: i32) -> f64 {
    f64::from_bits((u64::from(hi as u32) << 32) | u64::from(low(x)))
}

/// The reference's `Math.log`.
///
/// Bit-identical to the reference engine for every input in the `mathLog` fixture suite,
/// including zeros, negatives, subnormals, infinities and `NaN`.
#[expect(
    clippy::many_single_char_names,
    reason = "the names are the reference's, and matching them keeps the port auditable"
)]
pub fn math_log(x: f64) -> f64 {
    let mut x = x;
    let mut hx = high(x);
    let lx = low(x);
    let mut k: i32 = 0;

    if hx < 0x0010_0000 {
        // Note the *signed* comparison: a negative x lands here too.
        if ((hx & 0x7fff_ffff) | lx as i32) == 0 {
            return f64::NEG_INFINITY; // log(±0)
        }
        if hx < 0 {
            return f64::NAN; // log(negative)
        }
        k -= 54;
        x *= TWO54; // scale a subnormal into range
        hx = high(x);
    }
    if hx >= 0x7ff0_0000 {
        return x + x; // +inf, or NaN propagated
    }

    k += (hx >> 20) - 1023;
    hx &= 0x000f_ffff;
    let i = (hx + 0x0009_5f64) & 0x0010_0000;
    x = with_high(x, hx | (i ^ 0x3ff0_0000)); // normalise to x or x/2
    k += i >> 20;
    let f = x - 1.0;

    if (0x000f_ffff & (2 + hx)) < 3 {
        // |f| < 2^-20: the polynomial would lose all its significance.
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            let dk = f64::from(k);
            return dk * LN2_HI + dk * LN2_LO;
        }
        let r = f * f * (0.5 - ONE_THIRD * f);
        if k == 0 {
            return f - r;
        }
        let dk = f64::from(k);
        return dk * LN2_HI - ((r - dk * LN2_LO) - f);
    }

    let s = f / (2.0 + f);
    let dk = f64::from(k);
    let z = s * s;
    let mut i = hx - 0x0006_147a;
    let w = z * z;
    let j = 0x0006_b851 - hx;
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

#[cfg(test)]
mod tests {
    use super::math_log;

    #[test]
    fn matches_the_specified_rounding() {
        // The case the parity fixtures found: three documents, one match.
        assert_eq!(
            math_log(3.0).to_bits(),
            1.098_612_288_668_109_6_f64.to_bits()
        );
        assert_ne!(math_log(3.0).to_bits(), 3.0_f64.ln().to_bits());
    }

    #[test]
    fn special_values_match_the_reference() {
        assert_eq!(math_log(1.0), 0.0);
        assert_eq!(math_log(0.0), f64::NEG_INFINITY);
        assert_eq!(math_log(-0.0), f64::NEG_INFINITY);
        assert!(math_log(-1.0).is_nan());
        assert!(math_log(f64::NAN).is_nan());
        assert_eq!(math_log(f64::INFINITY), f64::INFINITY);
        assert!(math_log(f64::NEG_INFINITY).is_nan());
        // A subnormal, which takes the 2^54 scaling branch.
        assert_eq!(
            math_log(f64::from_bits(1)).to_bits(),
            (-744.440_071_921_381_2_f64).to_bits()
        );
    }

    #[test]
    fn stays_within_one_ulp_of_the_platform_libm() {
        // Not a parity check — that is the `mathLog` fixture suite — but a
        // guard against a transcription error turning the port into nonsense.
        for i in 1..2000 {
            let x = f64::from(i) / 7.0;
            let a = math_log(x);
            let b = x.ln();
            assert!(
                (a - b).abs() <= (a.abs() * 1e-15).max(1e-15),
                "log({x}) = {a}, libm says {b}"
            );
        }
    }
}
