//! Verbora's specified natural logarithm.
//!
//! The user-facing contract lives on [`natural_log`] itself, because this
//! module is private and a private module's `//!` prose never reaches the
//! rendered documentation. What stays here is the note to whoever edits the
//! constants below.
// The constants are given as bit patterns rather than decimal literals. fdlibm's
// source annotates every one of them with its hex encoding precisely because the
// decimal spelling is a lossy rendering of the intended double, and a truncated
// decimal — which is what a "this literal has excessive precision" suggestion
// produces — is a silently different function. Each `from_bits` below is the hex
// comment from `e_log.c`, verified to round-trip the decimal the C source also
// carries.

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

/// The high 32 bits of `x`, as fdlibm's signed `int32_t`.
#[inline]
#[expect(
    clippy::cast_possible_wrap,
    reason = "fdlibm reads the high word signed"
)]
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

/// The natural logarithm of `x`, computed identically on every platform.
///
/// This is the function [`TfIdf::idf`](crate::TfIdf::idf) is defined in terms
/// of; call it to reproduce a score by hand.
///
/// # Why this is not `f64::ln`
///
/// Every score this crate produces is `1 + ln(…)` summed over query terms, so
/// the logarithm *is* the numeric contract. `f64::ln` delegates to the platform
/// `libm`, which is not specified by Rust, differs between platforms, and
/// differs between versions of the same platform's C library. Two builds of the
/// same program can therefore disagree in the last bit of every score, which
/// makes a stored score, a regression fixture and a cross-machine comparison
/// all unreliable for reasons the caller cannot see.
///
/// Verbora specifies the function instead. This is Sun Microsystems'
/// `__ieee754_log` from **fdlibm** (1993, freely distributable; the basis of
/// FreeBSD's `msun` and, through it, of much of the libm family), implemented
/// in safe Rust. It has no platform inputs at all — only IEEE 754 double
/// arithmetic, which Rust does specify — so the same input yields the same 64
/// bits on every target this crate builds for.
///
/// # Accuracy, stated rather than implied
///
/// fdlibm documents its own error bound as **strictly under 1 ulp**, and that
/// is the bound Verbora publishes. It is *not* a claim of correct rounding, and
/// the difference is observable at the smallest input this crate routinely
/// reaches:
///
/// ```text
/// ln 3 = 1.09861228866810969139524523692252570464749055782…
///   nearest double        1.0986122886681098   (0x3FF193EA7AAD030B)
///   natural_log(3.0)      1.0986122886681096   (0x3FF193EA7AAD030A)   1 ulp low
/// ```
///
/// A three-document corpus with one match produces exactly `1 + ln 3`, so this
/// is not a corner case. It is within the documented bound, it is identical on
/// every platform, and it is pinned by a test that computes the true value
/// independently, from its decimal expansion, rather than from anything this
/// crate emits.
///
/// # What is implemented
///
/// `__ieee754_log` verbatim: argument reduction to `x = 2^k · (1 + f)` with
/// `√2/2 < 1+f < √2`, the `|f| < 2^-20` short path, and the degree-7 minimax
/// polynomial in `s = f/(2+f)`. Every intermediate keeps fdlibm's
/// parenthesisation — re-associating
/// `dk*ln2_hi - ((hfsq - (s*(hfsq+R) + dk*ln2_lo)) - f)` is algebraically free
/// and numerically not, and the published error bound is a property of the
/// written association. The bit patterns are manipulated as `i32`/`u32`
/// halves, matching the C; in particular the high word is **signed**, which is
/// what makes `log(-1)` take the subnormal branch and return `NaN` rather than
/// falling through to the infinity test.
///
/// # Special values
///
/// | `x` | result |
/// |---|---|
/// | `1.0` | `0.0` exactly |
/// | `+0.0`, `-0.0` | `f64::NEG_INFINITY` |
/// | any `x < 0.0` | `f64::NAN` |
/// | `f64::NAN` | `f64::NAN` |
/// | `f64::INFINITY` | `f64::INFINITY` |
/// | `f64::NEG_INFINITY` | `f64::NAN` |
///
/// No score this crate computes can reach any of those rows: the argument of
/// every logarithm it evaluates is `n / (1 + df)` with `n >= 1` and
/// `0 <= df <= n`, which is always positive and finite. They are specified
/// because the function is public, not because the crate depends on them.
///
/// # Examples
///
/// ```
/// use verbora_tfidf::natural_log;
///
/// assert_eq!(natural_log(1.0), 0.0);
/// assert_eq!(natural_log(0.0), f64::NEG_INFINITY);
/// assert!(natural_log(-1.0).is_nan());
///
/// // ln 3 = 1.09861228866810969139…, and this is the double one ulp below the
/// // nearest one — inside fdlibm's documented sub-ulp bound.
/// assert_eq!(natural_log(3.0), 1.0986122886681096);
/// ```
#[must_use]
#[expect(
    clippy::many_single_char_names,
    reason = "the names are fdlibm's, and matching them keeps the port auditable"
)]
pub fn natural_log(x: f64) -> f64 {
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
    use super::natural_log;

    /// `ln(x)` for a grid of inputs, given as **decimal expansions of the true
    /// mathematical value** to 30 significant digits.
    ///
    /// The digits come from the transcendental value, not from this crate and
    /// not from any `libm`: each was produced by arbitrary-precision evaluation
    /// of `ln` and is written out here so the expectation can be checked by
    /// hand against any table of logarithms. Rust's `str -> f64` parse is
    /// correctly rounded, so parsing one of these strings yields exactly the
    /// double nearest the true value.
    ///
    /// The inputs are the ratios `n / (1 + df)` that [`crate::TfIdf::idf`]
    /// evaluates for small corpora, plus the two boundaries of fdlibm's
    /// argument reduction.
    const TRUE_LN: &[(f64, &str)] = &[
        (2.0, "0.693147180559945309417232121458"),
        (3.0, "1.09861228866810969139524523692"),
        (4.0, "1.38629436111989061883446424292"),
        (5.0, "1.60943791243410037460075933323"),
        (10.0, "2.30258509299404568401799145468"),
        (0.5, "-0.693147180559945309417232121458"),
        (1.5, "0.405465108108164381978013115464"),
        (2.5, "0.916290731874155065183527211768"),
        // 4 / 3: three documents, one of which lacks the term.
        (1.333_333_333_333_333_3, "0.287682072451780871928067774736"),
        // 6 / 5.
        (1.2, "0.182321556793954589204283870983"),
        // 1 / 2^-1074, the smallest positive subnormal, which takes the 2^54
        // scaling branch.
        (5e-324, "-744.440071921381262314107298446"),
    ];

    /// The distance from `a` to `b` counted in representable doubles.
    ///
    /// Both arguments are finite and share a sign in every use below, so the
    /// monotone `to_bits` ordering of same-signed doubles makes this exact.
    fn ulps_between(a: f64, b: f64) -> u64 {
        assert!(a.is_finite() && b.is_finite(), "{a} or {b} is not finite");
        assert!(
            a.is_sign_positive() == b.is_sign_positive(),
            "{a} and {b} differ in sign"
        );
        let (x, y) = (a.to_bits(), b.to_bits());
        x.max(y) - x.min(y)
    }

    #[test]
    fn fixtures_are_within_one_ulp_of_the_true_value() {
        for (x, expansion) in TRUE_LN {
            let truth: f64 = expansion
                .parse()
                .expect("every fixture is a decimal literal");
            let got = natural_log(*x);
            assert!(
                ulps_between(got, truth) <= 1,
                "ln({x}) = {got:?}, nearest double to the true value is {truth:?}"
            );
        }
    }

    /// The one input whose sub-ulp error the module documentation quotes, held
    /// to the exact bits so the published claim cannot drift.
    #[test]
    fn ln_three_is_the_double_below_the_nearest_one() {
        let truth: f64 = "1.09861228866810969139524523692"
            .parse()
            .expect("a decimal literal");
        assert_eq!(truth, 1.0986122886681098, "nearest double to ln 3");
        assert_eq!(natural_log(3.0), 1.0986122886681096);
        assert_eq!(
            natural_log(3.0).to_bits() + 1,
            truth.to_bits(),
            "exactly one ulp low"
        );
    }

    #[test]
    fn special_values_match_the_documented_table() {
        assert_eq!(natural_log(1.0), 0.0);
        assert_eq!(natural_log(0.0), f64::NEG_INFINITY);
        assert_eq!(natural_log(-0.0), f64::NEG_INFINITY);
        assert!(natural_log(-1.0).is_nan());
        assert!(natural_log(f64::MIN).is_nan());
        assert!(natural_log(f64::NAN).is_nan());
        assert_eq!(natural_log(f64::INFINITY), f64::INFINITY);
        assert!(natural_log(f64::NEG_INFINITY).is_nan());
    }

    /// Every ratio an idf can evaluate for a corpus of up to 128 documents,
    /// enumerated rather than sampled: `n / (1 + df)` for `1 <= n <= 128` and
    /// `0 <= df <= n`.
    ///
    /// Two claims are checked over all 8,384 of them. The result must be finite
    /// and non-`NaN` — the property [`crate::TfIdf::idf`]'s "no `NaN` escapes"
    /// guarantee rests on — and it must agree with the platform `libm` to
    /// within 2 ulp, which is the sum of the two implementations' documented
    /// sub-ulp bounds. The second is a transcription guard: a typo in one of
    /// the polynomial constants would blow straight through it.
    #[test]
    fn every_reachable_idf_ratio_is_finite_and_within_two_ulp_of_libm() {
        let mut checked = 0u32;
        for n in 1..=128u32 {
            for df in 0..=n {
                let x = f64::from(n) / (1.0 + f64::from(df));
                let got = natural_log(x);
                assert!(got.is_finite(), "ln({x}) = {got:?}");
                let libm = x.ln();
                if got == 0.0 || libm == 0.0 {
                    assert_eq!(got, libm, "ln({x})");
                } else {
                    assert!(
                        ulps_between(got, libm) <= 2,
                        "ln({x}) = {got:?}, libm says {libm:?}"
                    );
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 8384);
    }

    /// The logarithm is non-decreasing, so `idf` is non-increasing in document
    /// frequency. Enumerated over the same ratio grid, in ascending order.
    #[test]
    fn the_logarithm_is_monotone_over_every_reachable_ratio() {
        let mut ratios: Vec<f64> = (1..=128u32)
            .flat_map(|n| (0..=n).map(move |df| f64::from(n) / (1.0 + f64::from(df))))
            .collect();
        ratios.sort_by(f64::total_cmp);
        for pair in ratios.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                natural_log(a) <= natural_log(b),
                "ln({a}) > ln({b}) but {a} <= {b}"
            );
        }
    }
}
