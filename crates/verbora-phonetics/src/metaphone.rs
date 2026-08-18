//! Original (Lawrence Philips, 1990) Metaphone, as the reference implements it.
//!
//! Ports the reference `metaphone`, which is an **ordered pipeline of
//! whole-string rewrites** rather than a single scan. The order is load-bearing
//! and two stages actively undo each other — see [`Metaphone::process`].
//!
//! # Why every rewrite is hand-written
//!
//! Not one of the twenty-one stages goes through a regular expression engine
//! here, and three of them *could not*:
//!
//! * [`Metaphone::dedup`] uses `/([^c])\1/g`, a **backreference**, which the
//!   Rust `regex` crate does not support.
//! * [`Metaphone::transform_t`] and [`Metaphone::transform_z`] use **non-global**
//!   patterns, so only the first occurrence is replaced; `Regex::replace_all` is
//!   the wrong translation.
//! * Six stages match an anchored alternation that **consumes** the context
//!   character (`/([^s]|^)(c)(h)/g` and friends), so matches cannot overlap and
//!   adjacent occurrences are skipped. A lookaround-based rewrite — which the
//!   `regex` crate also cannot express — would produce different output.
//!
//! Every rule is a bounded-context rewrite over a byte class, so a hand-written
//! scanner is both exact and faster than a regex engine would be.

use crate::pipe::Pipe;
use crate::units::{
    Unit, clamp_take, coerce_or_default, eq_ascii_slice, is_trim_unit, trim_units, uppercase_utf16,
    utf16_to_lowercase,
};

/// Original Metaphone.
///
/// ```
/// use verbora_phonetics::Metaphone;
///
/// let metaphone = Metaphone::new();
/// assert_eq!(metaphone.process("phonetics"), "FNTKS");
/// assert_eq!(metaphone.process("fonetix"), "FNTKS");
/// assert!(metaphone.compare("phonetics", "fonetix"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Metaphone;

/// The vowel class every Metaphone pattern uses: `[aeiou]`, lowercase only.
///
/// None of the reference's patterns carry the `/i` flag. `process` lowercases
/// first, so this only shows up when the stage methods are called directly —
/// but they are public API, and the reference's own spec exercises them.
#[inline]
fn is_vowel<U: Unit>(u: U) -> bool {
    matches!(u.to_ascii(), Some(b'a' | b'e' | b'i' | b'o' | b'u'))
}

impl Metaphone {
    /// Creates a Metaphone encoder. It holds no state; the type exists to
    /// mirror the reference `Metaphone`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` with the default maximum length of 32.
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        self.process_with(token, None)
    }

    /// Encodes `token`, honouring the reference's `maxLength` coercion.
    ///
    /// `max_length` is `Option<f64>` because the argument is a reference number
    /// and its odd values are observable: `maxLength || 32` sends `0`, `NaN` and
    /// `None` to the default, while `Some(-1.0)` reaches `String#substring` and
    /// yields `""`.
    ///
    /// # The result can be longer than `max_length`
    ///
    /// The reference truncates *before* uppercasing, and case mapping can grow a
    /// string: `process("ß".repeat(40), 3)` is `"SSSSSS"` — six characters for a
    /// budget of three. Do not assert an upper bound on the length here.
    #[must_use]
    pub fn process_with(&self, token: &str, max_length: Option<f64>) -> String {
        // ASCII tokens: byte-wise case folding equals Unicode lowercasing, so
        // the pooled pipeline folds straight into reused scratch — the
        // returned `String` is the call's only allocation. Non-ASCII tokens
        // keep the full Unicode fold first (which can itself produce ASCII,
        // e.g. the Kelvin sign — that rare case still reaches the ASCII
        // pipeline below, exactly as before).
        if token.is_ascii() {
            return pipeline_ascii_pooled(token, max_length, |window| {
                let mut out = window.to_vec();
                out.make_ascii_uppercase();
                String::from_utf8(out).expect("ASCII in, ASCII out")
            });
        }
        let lower = utf16_to_lowercase(token);
        if lower.is_ascii() {
            return pipeline_ascii_pooled(&lower, max_length, |window| {
                let mut out = window.to_vec();
                out.make_ascii_uppercase();
                String::from_utf8(out).expect("ASCII in, ASCII out")
            });
        }
        let units: Vec<u16> = lower.encode_utf16().collect();
        let out = uppercase_utf16(&pipeline_owned(units, max_length));
        String::from_utf16_lossy(&out)
    }

    /// Like [`Self::process_with`], but returns UTF-16 code units.
    ///
    /// Truncation happens at a code-unit boundary, which can fall between the
    /// two halves of a surrogate pair: `process("😀", 1)` is a lone high
    /// surrogate in the reference. A Rust `String` cannot hold one, so
    /// [`Self::process_with`] renders it as `U+FFFD`; this method returns the
    /// exact sequence.
    #[must_use]
    pub fn process_utf16(&self, token: &str, max_length: Option<f64>) -> Vec<u16> {
        if token.is_ascii() {
            return pipeline_ascii_pooled(token, max_length, |window| {
                window
                    .iter()
                    .map(|&b| u16::from(b.to_ascii_uppercase()))
                    .collect()
            });
        }
        let lower = utf16_to_lowercase(token);
        if lower.is_ascii() {
            return pipeline_ascii_pooled(&lower, max_length, |window| {
                window
                    .iter()
                    .map(|&b| u16::from(b.to_ascii_uppercase()))
                    .collect()
            });
        }
        let units: Vec<u16> = lower.encode_utf16().collect();
        uppercase_utf16(&pipeline_owned(units, max_length))
    }

    /// Whether two strings share a Metaphone code, at the default length.
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }

    // -- prototype methods, exposed because the reference exposes them ----------

    /// `token.replace(/([^c])\1/g, '$1')`.
    ///
    /// Collapses each non-overlapping **pair** of identical adjacent code units,
    /// which is not the same as collapsing runs: `dedup("aaa") == "aa"`,
    /// `dedup("aaaa") == "aa"`, `dedup("aaaaa") == "aaa"`. The exemption is for
    /// lowercase `c` only — the class is `[^c]` with no `/i` — so `dedup("CC")`
    /// is `"C"` while `dedup("cc")` is `"cc"`.
    #[must_use]
    pub fn dedup(&self, token: &str) -> String {
        run(token, s_dedup, s_dedup)
    }

    /// `/^(kn|gn|pn|ae|wr)/` — drops exactly one leading code unit.
    #[must_use]
    pub fn drop_initial_letters(&self, token: &str) -> String {
        run(token, s_drop_initial_letters, s_drop_initial_letters)
    }

    /// `token.replace(/mb$/, 'm')`.
    #[must_use]
    pub fn drop_b_after_m_at_end(&self, token: &str) -> String {
        run(token, s_drop_b_after_m_at_end, s_drop_b_after_m_at_end)
    }

    /// The four-step `c` rewrite, including its stray `.trim()`.
    ///
    /// Step one is `/([^s]|^)(c)(h)/g`, whose leading group **consumes** the
    /// preceding character. Consecutive digraphs are therefore not all rewritten
    /// in one pass: `c_transform("chch") == "xhkh"` (the second `ch` falls
    /// through to the final `c -> k` step) and `c_transform("schch") == "skhxh"`.
    ///
    /// Step one also ends with `.trim()`, which strips whitespace from **both
    /// ends of the whole token** in the middle of the pipeline. It follows
    /// the reference's definition, which includes `U+FEFF` — Rust's [`str::trim`]
    /// does not — so `c_transform("\u{FEFF}abc\u{FEFF}") == "abk"`.
    #[must_use]
    pub fn c_transform(&self, token: &str) -> String {
        run(token, s_c_transform, s_c_transform)
    }

    /// `/d(ge|gy|gi)/g -> 'j$1'`, then `/d/g -> 't'`.
    #[must_use]
    pub fn d_transform(&self, token: &str) -> String {
        run(token, s_d_transform, s_d_transform)
    }

    /// `/gh(^$|[^aeiou])/g -> 'h$1'`, then `/g(n|ned)$/g -> '$1'`.
    ///
    /// The `^$` alternative in the first pattern is **dead**: it would require
    /// the position after `gh` to be simultaneously the start and the end of the
    /// string. A word-final `gh` is therefore left alone here and becomes `f`
    /// later, in [`Self::transform_g`] — which is how `tough` encodes as `TF`.
    #[must_use]
    pub fn drop_g(&self, token: &str) -> String {
        run(token, s_drop_g, s_drop_g)
    }

    /// `/gh/g -> 'f'`, `/([^g]|^)(g)(i|e|y)/g -> '$1j$3'`, `/gg/g -> 'g'`,
    /// `/g/g -> 'k'`.
    ///
    /// The second pattern consumes the preceding character, so `ggi` never
    /// becomes `gji`: `transform_g("ggi") == "ki"`.
    #[must_use]
    pub fn transform_g(&self, token: &str) -> String {
        run(token, s_transform_g, s_transform_g)
    }

    /// `token.replace(/([aeiou])h([^aeiou]|$)/g, '$1$2')`.
    ///
    /// The trailing character is consumed, so matches cannot overlap:
    /// `drop_h("ohhoh") == "oho"` and `drop_h("ahaha")` is unchanged.
    #[must_use]
    pub fn drop_h(&self, token: &str) -> String {
        run(token, s_drop_h, s_drop_h)
    }

    /// `token.replace(/ck/g, 'k')`.
    #[must_use]
    pub fn transform_ck(&self, token: &str) -> String {
        run(token, s_ck, s_ck)
    }

    /// `token.replace(/ph/g, 'f')`.
    #[must_use]
    pub fn transform_ph(&self, token: &str) -> String {
        run(token, s_ph, s_ph)
    }

    /// `token.replace(/q/g, 'k')`.
    #[must_use]
    pub fn transform_q(&self, token: &str) -> String {
        run(token, s_q, s_q)
    }

    /// `token.replace(/s(h|io|ia)/g, 'x$1')`.
    #[must_use]
    pub fn transform_s(&self, token: &str) -> String {
        run(token, s_s_soft, s_s_soft)
    }

    /// `/t(ia|io)/g -> 'x$1'`, then `/th/ -> '0'`.
    ///
    /// The second pattern is **not global**: only the first `th` becomes `0`, so
    /// `transform_t("thth") == "0th"`.
    #[must_use]
    pub fn transform_t(&self, token: &str) -> String {
        run(token, s_transform_t, s_transform_t)
    }

    /// `token.replace(/tch/g, 'ch')`.
    #[must_use]
    pub fn drop_t(&self, token: &str) -> String {
        run(token, s_tch, s_tch)
    }

    /// `token.replace(/v/g, 'f')`.
    #[must_use]
    pub fn transform_v(&self, token: &str) -> String {
        run(token, s_v, s_v)
    }

    /// `token.replace(/^wh/, 'w')`.
    #[must_use]
    pub fn transform_wh(&self, token: &str) -> String {
        run(token, s_wh, s_wh)
    }

    /// `token.replace(/w([^aeiou]|$)/g, '$1')`. Non-overlapping:
    /// `drop_w("www") == "w"`.
    #[must_use]
    pub fn drop_w(&self, token: &str) -> String {
        run(token, s_drop_w, s_drop_w)
    }

    /// `/^x/ -> 's'`, then `/x/g -> 'ks'`.
    ///
    /// Because [`Self::process`] runs this **after** [`Self::c_transform`], a
    /// word-initial `ch` that became `xh` is turned into `sh` here. That is the
    /// reference's known bug (`chemical` encodes as `SHMKL`, not `KMKL`), and it
    /// is reproduced deliberately.
    #[must_use]
    pub fn transform_x(&self, token: &str) -> String {
        run(token, s_transform_x, s_transform_x)
    }

    /// `token.replace(/y([^aeiou]|$)/g, '$1')`. Non-overlapping:
    /// `drop_y("yyy") == "y"`.
    #[must_use]
    pub fn drop_y(&self, token: &str) -> String {
        run(token, s_drop_y, s_drop_y)
    }

    /// `token.replace(/z/, 's')` — **not global**: `transform_z("zzz") == "szz"`.
    #[must_use]
    pub fn transform_z(&self, token: &str) -> String {
        run(token, s_first_z, s_first_z)
    }

    /// `token.charAt(0) + token.substr(1).replace(/[aeiou]/g, '')`.
    ///
    /// The first **code unit** is kept verbatim, even when it is a vowel or half
    /// of a surrogate pair.
    #[must_use]
    pub fn drop_vowels(&self, token: &str) -> String {
        run(token, s_drop_vowels, s_drop_vowels)
    }
}

/// Runs one stage over a `&str` and renders the result.
///
/// The stage is passed twice, once per unit width: a generic `fn` item can be
/// instantiated at both, whereas a closure has a single inferred signature and
/// could not serve both branches.
///
/// Used only by the public stage methods. No individual stage can orphan a
/// surrogate — each either copies code units through or deletes ASCII ones — so
/// the conversion back is lossless here. Only [`Metaphone::process`], which
/// truncates, needs the UTF-16 form.
fn run(token: &str, stage8: fn(&mut Pipe<u8>), stage16: fn(&mut Pipe<u16>)) -> String {
    if token.is_ascii() {
        let mut pipe = Pipe::new(token.as_bytes());
        stage8(&mut pipe);
        String::from_utf8(pipe.finish()).expect("ASCII in, ASCII out")
    } else {
        let units: Vec<u16> = token.encode_utf16().collect();
        let mut pipe = Pipe::new(&units);
        stage16(&mut pipe);
        String::from_utf16_lossy(&pipe.finish())
    }
}

// -- stages: one per the reference prototype method ----------------------------

/// `dedup`
fn s_dedup<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_dedup);
}

/// `dropInitialLetters`
fn s_drop_initial_letters<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_drop_initial_letters);
}

/// `dropBafterMAtEnd`
fn s_drop_b_after_m_at_end<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_drop_b_after_m_at_end);
}

/// `cTransform`
fn s_c_transform<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_ch_to_xh);
    p.apply(r_trim);
    p.apply(r_c_soft);
}

/// `dTransform`
fn s_d_transform<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_dge);
    p.apply(r_d_to_t);
}

/// `dropG`
fn s_drop_g<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_gh_before_consonant);
    p.apply(r_drop_final_g);
}

/// `transformG`
fn s_transform_g<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_gh_to_f);
    p.apply(r_g_soft);
    p.apply(r_gg);
    p.apply(r_g_to_k);
}

/// `dropH`
fn s_drop_h<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_drop_h);
}

/// `transformCK`
fn s_ck<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_ck);
}

/// `transformPH`
fn s_ph<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_ph);
}

/// `transformQ`
fn s_q<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_q);
}

/// `transformS`
fn s_s_soft<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_s_soft);
}

/// `transformX`
fn s_transform_x<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_initial_x);
    p.apply(r_x_to_ks);
}

/// `transformT`
fn s_transform_t<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_tia);
    p.apply(r_first_th);
}

/// `dropT`
fn s_tch<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_tch);
}

/// `transformV`
fn s_v<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_v);
}

/// `transformWH`
fn s_wh<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_wh);
}

/// `dropW`
fn s_drop_w<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_drop_w);
}

/// `dropY`
fn s_drop_y<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_drop_y);
}

/// `transformZ`
fn s_first_z<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_first_z);
}

/// `dropVowels`
fn s_drop_vowels<U: Unit>(p: &mut Pipe<U>) {
    p.apply(r_drop_vowels);
}

// ---------------------------------------------------------------------------
// The gated pipeline driver
// ---------------------------------------------------------------------------
//
// [`pipeline`] computes exactly the same function as [`pipeline_reference`] —
// the 21 stages in the reference's order — but skips the stages that provably
// cannot rewrite the current text, and fuses a few adjacent sub-passes whose
// composition is provable. Parity is enforced mechanically: the test module
// runs both pipelines differentially over the benchmark word lists, large
// random corpora, exhaustive short-string sweeps and hand-built adversarial
// cases (see `tests::parity`).

/// The presence-mask bit for a lowercase ASCII letter.
const fn lbit(b: u8) -> u32 {
    1 << (b - b'a')
}

/// The mask bits for the vowel class `[aeiou]`.
const VOWEL_BITS: u32 = lbit(b'a') | lbit(b'e') | lbit(b'i') | lbit(b'o') | lbit(b'u');

/// One pass over `units`: the letter-presence mask, plus whether any adjacent
/// pair of equal units (other than lowercase `c`) exists — [`r_dedup`]'s exact
/// firing condition. The pair test covers **all** units, ASCII or not, because
/// `dedup` collapses any equal adjacent pair whose unit is not lowercase `c`.
fn scan_letters<U: Unit>(units: &[U]) -> (u32, bool) {
    let mut mask = 0u32;
    let mut dup = false;
    let mut prev: Option<U> = None;
    for &u in units {
        if let Some(b) = u.to_ascii() {
            if b.is_ascii_lowercase() {
                mask |= lbit(b);
            }
        }
        if prev == Some(u) && !u.eq_ascii(b'c') {
            dup = true;
        }
        prev = Some(u);
    }
    (mask, dup)
}

/// The letter-presence mask alone, for rebuilds after a mutating stage.
fn letter_mask<U: Unit>(units: &[U]) -> u32 {
    let mut mask = 0u32;
    for &u in units {
        if let Some(b) = u.to_ascii() {
            if b.is_ascii_lowercase() {
                mask |= lbit(b);
            }
        }
    }
    mask
}

/// Which scratch buffer currently holds the pipeline text.
enum Held {
    /// Scratch buffer A.
    A,
    /// Scratch buffer B.
    B,
}

/// The skip-aware replacement for [`Pipe`] inside [`run_pipeline`].
///
/// Holds the text as a `start..end` **window** over either the borrowed input
/// or one of two lazily-allocated scratch buffers. Anchored deletions
/// (`dropInitialLetters`, `/mb$/`, the mid-pipeline `trim`) only move the
/// window; a skipped stage touches nothing at all. Only a stage that actually
/// fires pays for a rewrite pass, and the letter mask is rebuilt from the
/// result so later gates stay a superset of the letters present.
struct Driver<'a, U> {
    /// Scratch buffer A (borrowed so callers can pool it across calls —
    /// the ASCII entry points reuse a thread-local pair, cutting per-call
    /// allocator traffic to the one output string; seeded with the
    /// lowercased input, so `held` starts at [`Held::A`]).
    a: &'a mut Vec<U>,
    /// Scratch buffer B.
    b: &'a mut Vec<U>,
    /// Which of the three buffers holds the current text.
    held: Held,
    /// Window start into the held buffer.
    start: usize,
    /// Window end into the held buffer.
    end: usize,
    /// A **superset** of the lowercase ASCII letters present in the window.
    /// Window-only edits shrink the text without rebuilding this, which is
    /// safe: gates use it as a necessary condition only.
    mask: u32,
}

impl<'a, U: Unit> Driver<'a, U> {
    /// Starts a pipeline over `input` with its precomputed letter mask.
    /// The current text window.
    fn cur(&self) -> &[U] {
        match self.held {
            Held::A => &self.a[self.start..self.end],
            Held::B => &self.b[self.start..self.end],
        }
    }

    /// Whether every letter in `bits` may be present.
    fn has_all(&self, bits: u32) -> bool {
        self.mask & bits == bits
    }

    /// Whether any letter in `bits` may be present.
    fn has_any(&self, bits: u32) -> bool {
        self.mask & bits != 0
    }

    /// Runs one rewrite rule into the free scratch buffer and rebuilds the
    /// letter mask from its output. Call only when the rule is known to fire;
    /// a skipped stage costs nothing.
    fn apply(&mut self, rule: fn(&[U], &mut Vec<U>)) {
        self.apply_last(rule);
        self.mask = letter_mask(self.cur());
    }

    /// [`Self::apply`] without the mask rebuild — for the pipeline's final
    /// stage, whose output no later gate reads.
    fn apply_last(&mut self, rule: fn(&[U], &mut Vec<U>)) {
        match self.held {
            Held::A => {
                let src = &self.a[self.start..self.end];
                self.b.clear();
                self.b.reserve(src.len() + 8);
                rule(src, self.b);
                self.held = Held::B;
                self.end = self.b.len();
            }
            Held::B => {
                let src = &self.b[self.start..self.end];
                self.a.clear();
                self.a.reserve(src.len() + 8);
                rule(src, self.a);
                self.held = Held::A;
                self.end = self.a.len();
            }
        }
        self.start = 0;
    }

    /// The reference `.trim()` as a window edit — exactly [`trim_units`]'s
    /// arithmetic, applied to `start..end` instead of a fresh slice.
    fn trim_window(&mut self) {
        let cur = self.cur();
        let lead = cur.iter().take_while(|&&u| is_trim_unit(u)).count();
        if lead == cur.len() {
            self.start = self.end;
            return;
        }
        let trail = cur.iter().rev().take_while(|&&u| is_trim_unit(u)).count();
        self.start += lead;
        self.end -= trail;
    }

    /// The final text window. Borrowed — callers copy it into their own
    /// output (uppercasing on the way, which they all did anyway), so the
    /// pooled scratch never leaves the driver.
    fn finish(self) -> &'a [U] {
        let Self {
            a,
            b,
            held,
            start,
            end,
            ..
        } = self;
        match held {
            Held::A => &a[start..end],
            Held::B => &b[start..end],
        }
    }
}

// -- gate seeks --------------------------------------------------------------
//
// Each seek answers "would this rule rewrite anything?" with the rule's own
// match predicates, tested at every position. That is exact for every rule
// here, including the context-consuming ones, by the **no-rewrite lemma**: a
// scan that never rewrites never consumes extra units, so it advances one unit
// at a time and evaluates its predicates at every position — precisely what
// the seek does. And every rewrite branch of every rule changes the text
// (each one substitutes a different letter or changes the length), so
// `seek == false` ⟺ the rule is the identity on this text.

/// Whether any unit equals the ASCII byte `b`.
fn seek_unit<U: Unit>(units: &[U], b: u8) -> bool {
    units.iter().any(|&u| u.eq_ascii(b))
}

/// Whether the adjacent pair `ab` occurs — [`r_ck`]/[`r_ph`]'s firing test.
fn seek_digraph<U: Unit>(units: &[U], a: u8, b: u8) -> bool {
    units
        .windows(2)
        .any(|w| w[0].eq_ascii(a) && w[1].eq_ascii(b))
}

/// Whether [`r_ch_to_xh`] fires: `^ch`, or `ch` after a non-`s` unit.
fn seek_ch_to_xh<U: Unit>(units: &[U]) -> bool {
    let n = units.len();
    if n >= 2 && units[0].eq_ascii(b'c') && units[1].eq_ascii(b'h') {
        return true;
    }
    (0..n).any(|i| {
        !units[i].eq_ascii(b's')
            && i + 2 < n
            && units[i + 1].eq_ascii(b'c')
            && units[i + 2].eq_ascii(b'h')
    })
}

/// Whether [`r_gh_before_consonant`] fires: `gh` before a non-vowel unit.
fn seek_gh_before_consonant<U: Unit>(units: &[U]) -> bool {
    let n = units.len();
    (0..n).any(|i| {
        units[i].eq_ascii(b'g')
            && i + 2 < n
            && units[i + 1].eq_ascii(b'h')
            && !is_vowel(units[i + 2])
    })
}

/// Whether [`r_drop_h`] fires: vowel + `h` + (non-vowel or end).
fn seek_drop_h<U: Unit>(units: &[U]) -> bool {
    let n = units.len();
    (0..n).any(|i| {
        is_vowel(units[i])
            && i + 1 < n
            && units[i + 1].eq_ascii(b'h')
            && (i + 2 == n || !is_vowel(units[i + 2]))
    })
}

/// Whether [`r_s_soft`] fires: `s` + (`h` | `io` | `ia`).
fn seek_s_soft<U: Unit>(units: &[U]) -> bool {
    let n = units.len();
    (0..n).any(|i| {
        units[i].eq_ascii(b's')
            && ((i + 1 < n && units[i + 1].eq_ascii(b'h'))
                || (i + 2 < n
                    && units[i + 1].eq_ascii(b'i')
                    && matches!(units[i + 2].to_ascii(), Some(b'o' | b'a'))))
    })
}

/// Whether [`r_t_fused`] fires: a `t(ia|io)` match or any `th`.
///
/// "Any `th` in the input" equals "any `th` in `r_tia`'s output" because
/// `r_tia` neither creates a `th` (its replacement `x$1` contains neither `t`
/// nor `h`) nor destroys one (a `t` before `h` is never a `t(ia|io)` match,
/// and consumed units are `i`/`a`/`o`, never `t`).
fn seek_t_fused<U: Unit>(units: &[U]) -> bool {
    let n = units.len();
    (0..n).any(|i| {
        units[i].eq_ascii(b't')
            && ((i + 2 < n
                && units[i + 1].eq_ascii(b'i')
                && matches!(units[i + 2].to_ascii(), Some(b'a' | b'o')))
                || (i + 1 < n && units[i + 1].eq_ascii(b'h')))
    })
}

/// Whether [`r_tch`] fires: the trigraph `tch`.
fn seek_tch<U: Unit>(units: &[U]) -> bool {
    let n = units.len();
    (0..n).any(|i| {
        units[i].eq_ascii(b't')
            && i + 2 < n
            && units[i + 1].eq_ascii(b'c')
            && units[i + 2].eq_ascii(b'h')
    })
}

/// Whether [`drop_before_consonant`] fires for `c`: `c` + (non-vowel or end).
fn seek_drop_before_consonant<U: Unit>(units: &[U], c: u8) -> bool {
    let n = units.len();
    (0..n).any(|i| units[i].eq_ascii(c) && (i + 1 == n || !is_vowel(units[i + 1])))
}

thread_local! {
    /// Per-thread scratch pair for the ASCII pipeline. Reusing these across
    /// calls cuts per-call allocator traffic to the one returned `String` —
    /// measured as roughly a quarter of the whole per-name cost on the
    /// competitive benchmark's mixed-name batches. Safe plain `RefCell`
    /// reuse: the rules never call back into `process*`, so the borrow is
    /// never re-entered, and each rayon worker gets its own pair.
    static ASCII_SCRATCH: std::cell::RefCell<(Vec<u8>, Vec<u8>)> =
        const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
}

/// The ASCII pipeline over pooled scratch: lowercases `token` directly into
/// scratch A (no intermediate `String`), runs the rule sequence, and hands
/// the final window to `emit`, which builds the caller's owned output —
/// the single allocation the call makes in steady state.
fn pipeline_ascii_pooled<R>(
    token: &str,
    max_length: Option<f64>,
    emit: impl FnOnce(&[u8]) -> R,
) -> R {
    debug_assert!(token.is_ascii());
    ASCII_SCRATCH.with(|cell| {
        let scratch = &mut *cell.borrow_mut();
        let (a, b) = (&mut scratch.0, &mut scratch.1);
        a.clear();
        a.extend(token.bytes().map(|x| x.to_ascii_lowercase()));
        let (mask, has_dup_pair) = scan_letters(a);
        let end = a.len();
        let d = Driver {
            a,
            b,
            held: Held::A,
            start: 0,
            end,
            mask,
        };
        emit(run_pipeline(d, has_dup_pair, max_length))
    })
}

/// [`pipeline`] over an owned, already-lowercased buffer: the buffer becomes
/// the driver's first scratch instead of being borrowed, so the case-folding
/// allocation the caller already paid doubles as the first rewrite target —
/// one allocation fewer per call than lowercase-then-borrow, with the
/// identical rule sequence (the driver never re-reads its original input
/// once a rewrite has fired, so seeding scratch A is observationally the
/// same as borrowing).
fn pipeline_owned<U: Unit>(mut lower: Vec<U>, max_length: Option<f64>) -> Vec<U> {
    let (mask, has_dup_pair) = scan_letters(&lower);
    let end = lower.len();
    let mut b = Vec::new();
    let d = Driver {
        a: &mut lower,
        b: &mut b,
        held: Held::A,
        start: 0,
        end,
        mask,
    };
    run_pipeline(d, has_dup_pair, max_length).to_vec()
}

/// The full 21-stage pipeline plus truncation, over one unit width.
///
/// Computes byte-for-byte what [`pipeline_reference`] computes — the parity
/// tests prove it mechanically — but gates every stage on an O(1) letter-mask
/// or anchor test plus an exact match seek, so a stage that cannot rewrite
/// costs nothing, and fuses five provably-composable sub-pass groups. The
/// **stage numbers** below are positions in the reference `Metaphone#process`
/// order, which [`pipeline_reference`] preserves verbatim:
///
/// | here | reference stages | how |
/// |---|---|---|
/// | `r_dedup` | 1 `dedup` | gated on the exact adjacent-pair flag from [`scan_letters`] |
/// | window bump | 2 `dropInitialLetters` | O(1) prefix test, drops one leading unit without copying |
/// | window cut | 3 `dropBafterMAtEnd` | O(1) `mb$` test, drops the `b` without copying |
/// | `r_ck` | 4 `transformCK` | gated `c`&`k` + seek |
/// | `r_ch_to_xh` | 5 `cTransform` (i) | gated `c`&`h` + seek |
/// | `trim_window` | 5 `cTransform` (ii) `.trim()` | window edit, [`trim_units`] arithmetic |
/// | `r_c_soft` | 5 `cTransform` (iii) | gated `c` + seek; itself the pre-existing 3-way fusion |
/// | [`r_d_fused`] | 6 `dTransform` (both passes) | gated `d` + seek |
/// | `r_gh_before_consonant` | 7 `dropG` (i) | gated `g`&`h` + seek |
/// | `r_drop_final_g` | 7 `dropG` (ii) | O(1) `gn$`/`gned$` suffix test |
/// | [`r_g_fused`] | 8 `transformG` (all four passes) | gated `g` + seek |
/// | `r_drop_h` | 9 `dropH` | gated `h`&vowel + seek |
/// | `r_ph` | 10 `transformPH` | gated `p`&`h` + seek |
/// | *(moved into [`r_final`])* | 11 `transformQ` | see below |
/// | `r_s_soft` | 12 `transformS` | gated `s`&(`h`\|`i`) + seek |
/// | [`r_x_fused`] | 13 `transformX` (both passes) | gated `x` + seek |
/// | [`r_t_fused`] | 14 `transformT` (both passes) | gated `t`&(`i`\|`h`) + seek |
/// | `r_tch` | 15 `dropT` | gated `t`&`c`&`h` + seek (dynamically dead: stage 5 clears `c`) |
/// | *(moved into [`r_final`])* | 16 `transformV` | see below |
/// | `r_wh` | 17 `transformWH` | O(1) `^wh` test |
/// | `r_drop_w` | 18 `dropW` | gated `w` + seek |
/// | `r_drop_y` | 19 `dropY` | gated `y` + seek |
/// | [`r_final`] | 11 `transformQ` + 16 `transformV` + 20 `transformZ` + 21 `dropVowels` | gated on vowel\|`q`\|`v`\|`z` |
///
/// Stages 11 (`q`→`k`) and 16 (`v`→`f`) commute with everything between their
/// reference slot and the end: they are per-unit 1:1 maps whose inputs (`q`,
/// `v`) and outputs (`k`, `f`) are all non-vowels and none of the letters any
/// intervening stage matches on (`s h i o a x t c 0 w y z`), so no intervening
/// match, context test or vowel test can tell them apart, and no intervening
/// stage creates or destroys a `q` or `v`. Stage 20 (first `z`→`s`) commutes
/// with 21 because vowel deletion preserves both every `z` and their order.
/// [`r_final`] therefore applies all three maps inside stage 21's single scan.
///
/// Truncation is unchanged: `substring(0, maxLengthNew)` after all stages.
fn run_pipeline<'a, U: Unit>(
    mut d: Driver<'a, U>,
    has_dup_pair: bool,
    max_length: Option<f64>,
) -> &'a [U] {
    let max = coerce_or_default(max_length, 32.0);

    // 1 dedup — `has_dup_pair` is its exact firing condition.
    if has_dup_pair {
        d.apply(r_dedup);
    }
    // 2 dropInitialLetters — front-anchored: a window bump, no copy.
    {
        let cur = d.cur();
        if cur.len() >= 2
            && INITIAL_PREFIXES
                .iter()
                .any(|p| eq_ascii_slice(&cur[..2], p))
        {
            d.start += 1;
        }
    }
    // 3 dropBafterMAtEnd — end-anchored: a window cut, no copy.
    {
        let cur = d.cur();
        let n = cur.len();
        if n >= 2 && eq_ascii_slice(&cur[n - 2..], b"mb") {
            d.end -= 1;
        }
    }
    // 4 transformCK
    if d.has_all(lbit(b'c') | lbit(b'k')) && seek_digraph(d.cur(), b'c', b'k') {
        d.apply(r_ck);
    }
    // 5 cTransform (i): ch -> xh
    if d.has_all(lbit(b'c') | lbit(b'h')) && seek_ch_to_xh(d.cur()) {
        d.apply(r_ch_to_xh);
    }
    // 5 cTransform (ii): the reference `.trim()` — a window edit, no copy.
    d.trim_window();
    // 5 cTransform (iii): cia/c(iey)/c — every `c` fires one of its branches.
    if d.has_any(lbit(b'c')) && seek_unit(d.cur(), b'c') {
        d.apply(r_c_soft);
    }
    // 6 dTransform, fused — every `d` fires one of its branches.
    if d.has_any(lbit(b'd')) && seek_unit(d.cur(), b'd') {
        d.apply(r_d_fused);
    }
    // 7 dropG (i): gh before consonant
    if d.has_all(lbit(b'g') | lbit(b'h')) && seek_gh_before_consonant(d.cur()) {
        d.apply(r_gh_before_consonant);
    }
    // 7 dropG (ii): g(n|ned)$ — the rule fires iff the text ends in `gn` or
    // `gned` (`rest` must be exactly `"n"` or `"ned"`), an O(1) suffix test.
    {
        let cur = d.cur();
        let n = cur.len();
        if (n >= 2 && eq_ascii_slice(&cur[n - 2..], b"gn"))
            || (n >= 4 && eq_ascii_slice(&cur[n - 4..], b"gned"))
        {
            d.apply(r_drop_final_g);
        }
    }
    // 8 transformG, fused — every `g` fires one of its branches.
    if d.has_any(lbit(b'g')) && seek_unit(d.cur(), b'g') {
        d.apply(r_g_fused);
    }
    // 9 dropH
    if d.has_any(lbit(b'h')) && d.has_any(VOWEL_BITS) && seek_drop_h(d.cur()) {
        d.apply(r_drop_h);
    }
    // 10 transformPH
    if d.has_all(lbit(b'p') | lbit(b'h')) && seek_digraph(d.cur(), b'p', b'h') {
        d.apply(r_ph);
    }
    // 11 transformQ runs inside `r_final` below.
    // 12 transformS
    if d.has_any(lbit(b's')) && d.has_any(lbit(b'h') | lbit(b'i')) && seek_s_soft(d.cur()) {
        d.apply(r_s_soft);
    }
    // 13 transformX, fused — every `x` fires one of its branches.
    if d.has_any(lbit(b'x')) && seek_unit(d.cur(), b'x') {
        d.apply(r_x_fused);
    }
    // 14 transformT, fused
    if d.has_any(lbit(b't')) && d.has_any(lbit(b'i') | lbit(b'h')) && seek_t_fused(d.cur()) {
        d.apply(r_t_fused);
    }
    // 15 dropT — dynamically dead after stage 5 clears every `c`, but kept:
    // the gate discovers that per word instead of relying on a hand proof.
    if d.has_all(lbit(b't') | lbit(b'c') | lbit(b'h')) && seek_tch(d.cur()) {
        d.apply(r_tch);
    }
    // 16 transformV runs inside `r_final` below.
    // 17 transformWH — front-anchored, O(1) test.
    {
        let cur = d.cur();
        if cur.len() >= 2 && cur[0].eq_ascii(b'w') && cur[1].eq_ascii(b'h') {
            d.apply(r_wh);
        }
    }
    // 18 dropW
    if d.has_any(lbit(b'w')) && seek_drop_before_consonant(d.cur(), b'w') {
        d.apply(r_drop_w);
    }
    // 19 dropY
    if d.has_any(lbit(b'y')) && seek_drop_before_consonant(d.cur(), b'y') {
        d.apply(r_drop_y);
    }
    // 20 transformZ + 21 dropVowels (+ deferred 11 and 16), one scan.
    if d.has_any(VOWEL_BITS | lbit(b'q') | lbit(b'v') | lbit(b'z')) {
        d.apply_last(r_final);
    }

    let out = d.finish();
    // `if (token.length >= maxLengthNew) token = token.substring(0, maxLengthNew)`
    // — the guard is redundant, since `substring` past the end is a no-op, but
    // the negative case still truncates to nothing.
    &out[..clamp_take(max, out.len())]
}

/// The original stage-by-stage pipeline, kept verbatim as the parity oracle.
///
/// This is the line-for-line reference-traceable form — each `s_*` stage reads
/// beside the reference method it ports — and the byte-exact specification
/// that [`pipeline`] is differentially tested against. Do not modify one
/// without the other.
///
/// The order is exactly `Metaphone#process`'s, including the two orderings
/// that look like mistakes and are: `transformX` runs after `cTransform`
/// (turning a word-initial `ch` into `sh`), and `dropG` runs before
/// `transformG` (leaving a word-final `gh` to become `f`).
#[cfg(test)]
fn pipeline_reference<U: Unit>(lower: &[U], max_length: Option<f64>) -> Vec<U> {
    let max = coerce_or_default(max_length, 32.0);
    let mut p = Pipe::new(lower);

    s_dedup(&mut p);
    s_drop_initial_letters(&mut p);
    s_drop_b_after_m_at_end(&mut p);
    s_ck(&mut p);
    s_c_transform(&mut p);
    s_d_transform(&mut p);
    s_drop_g(&mut p);
    s_transform_g(&mut p);
    s_drop_h(&mut p);
    s_ph(&mut p);
    s_q(&mut p);
    s_s_soft(&mut p);
    s_transform_x(&mut p);
    s_transform_t(&mut p);
    s_tch(&mut p);
    s_v(&mut p);
    s_wh(&mut p);
    s_drop_w(&mut p);
    s_drop_y(&mut p);
    s_first_z(&mut p);
    s_drop_vowels(&mut p);

    let mut out = p.finish();
    // `if (token.length >= maxLengthNew) token = token.substring(0, maxLengthNew)`
    // — the guard is redundant, since `substring` past the end is a no-op, but
    // the negative case still truncates to nothing.
    out.truncate(clamp_take(max, out.len()));
    out
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// `/([^c])\1/g -> '$1'`
fn r_dedup<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let mut i = 0;
    while i < input.len() {
        let c = input[i];
        out.push(c);
        // A matched pair consumes BOTH units, so an odd run is only halved.
        i += usize::from(i + 1 < input.len() && input[i + 1] == c && !c.eq_ascii(b'c')) + 1;
    }
}

/// `.trim()` — the reference's definition, not Rust's.
fn r_trim<U: Unit>(input: &[U], out: &mut Vec<U>) {
    out.extend_from_slice(trim_units(input));
}

/// The `dropInitialLetters` prefixes: `/^(kn|gn|pn|ae|wr)/`.
///
/// Shared by [`r_drop_initial_letters`] and [`pipeline`]'s O(1) gate for it.
const INITIAL_PREFIXES: [&[u8]; 5] = [b"kn", b"gn", b"pn", b"ae", b"wr"];

/// `/^(kn|gn|pn|ae|wr)/` -> `substr(1)`
fn r_drop_initial_letters<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let drop = input.len() >= 2
        && INITIAL_PREFIXES
            .iter()
            .any(|p| eq_ascii_slice(&input[..2], p));
    out.extend_from_slice(if drop { &input[1..] } else { input });
}

/// `/mb$/ -> 'm'`
fn r_drop_b_after_m_at_end<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    if n >= 2 && eq_ascii_slice(&input[n - 2..], b"mb") {
        out.extend_from_slice(&input[..n - 1]);
    } else {
        out.extend_from_slice(input);
    }
}

/// `/([^s]|^)(c)(h)/g -> '$1x$3'`
///
/// The alternation is ordered, so `[^s]` is tried before `^` at every start
/// position; the winning alternative consumes the context character, which is
/// why adjacent `ch` pairs are not all rewritten.
fn r_ch_to_xh<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if !input[i].eq_ascii(b's')
            && i + 2 < n
            && input[i + 1].eq_ascii(b'c')
            && input[i + 2].eq_ascii(b'h')
        {
            out.push(input[i]);
            out.push(U::from_ascii(b'x'));
            out.push(input[i + 2]);
            i += 3;
        } else if i == 0 && n >= 2 && input[0].eq_ascii(b'c') && input[1].eq_ascii(b'h') {
            out.push(U::from_ascii(b'x'));
            out.push(input[1]);
            i = 2;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
}

/// `/cia/g -> 'xia'`, `/c(i|e|y)/g -> 's$1'`, `/c/g -> 'k'`, fused.
///
/// The three passes are fusable because none of their replacements (`xia`,
/// `s`+vowel, `k`) contains a `c`, so no pass can create work for a later one,
/// and each pass scans left to right without overlap. Trying `cia` first
/// preserves the reference's ordering.
fn r_c_soft<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if !input[i].eq_ascii(b'c') {
            out.push(input[i]);
            i += 1;
            continue;
        }
        if i + 2 < n && input[i + 1].eq_ascii(b'i') && input[i + 2].eq_ascii(b'a') {
            out.push(U::from_ascii(b'x'));
            out.push(input[i + 1]);
            out.push(input[i + 2]);
            i += 3;
        } else if i + 1 < n && matches!(input[i + 1].to_ascii(), Some(b'i' | b'e' | b'y')) {
            out.push(U::from_ascii(b's'));
            out.push(input[i + 1]);
            i += 2;
        } else {
            out.push(U::from_ascii(b'k'));
            i += 1;
        }
    }
}

/// `/d(ge|gy|gi)/g -> 'j$1'`
fn r_dge<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if input[i].eq_ascii(b'd')
            && i + 2 < n
            && input[i + 1].eq_ascii(b'g')
            && matches!(input[i + 2].to_ascii(), Some(b'e' | b'y' | b'i'))
        {
            out.push(U::from_ascii(b'j'));
            out.push(input[i + 1]);
            out.push(input[i + 2]);
            i += 3;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
}

/// `/d/g -> 't'`
fn r_d_to_t<U: Unit>(input: &[U], out: &mut Vec<U>) {
    replace_unit(input, out, b'd', b't');
}

/// `/gh(^$|[^aeiou])/g -> 'h$1'`; the `^$` alternative is unreachable.
fn r_gh_before_consonant<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if input[i].eq_ascii(b'g')
            && i + 2 < n
            && input[i + 1].eq_ascii(b'h')
            && !is_vowel(input[i + 2])
        {
            out.push(U::from_ascii(b'h'));
            out.push(input[i + 2]);
            i += 3;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
}

/// `/g(n|ned)$/g -> '$1'`
fn r_drop_final_g<U: Unit>(input: &[U], out: &mut Vec<U>) {
    for p in 0..input.len() {
        if input[p].eq_ascii(b'g') {
            let rest = &input[p + 1..];
            // The alternation tries `n` before `ned`, but both are anchored to
            // the end, so at most one can match at any start position.
            if eq_ascii_slice(rest, b"n") || eq_ascii_slice(rest, b"ned") {
                out.extend_from_slice(&input[..p]);
                out.extend_from_slice(rest);
                return;
            }
        }
    }
    out.extend_from_slice(input);
}

/// `/gh/g -> 'f'`
fn r_gh_to_f<U: Unit>(input: &[U], out: &mut Vec<U>) {
    replace_digraph(input, out, b"gh", b'f');
}

/// `/([^g]|^)(g)(i|e|y)/g -> '$1j$3'`
fn r_g_soft<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if !input[i].eq_ascii(b'g')
            && i + 2 < n
            && input[i + 1].eq_ascii(b'g')
            && matches!(input[i + 2].to_ascii(), Some(b'i' | b'e' | b'y'))
        {
            out.push(input[i]);
            out.push(U::from_ascii(b'j'));
            out.push(input[i + 2]);
            i += 3;
        } else if i == 0
            && n >= 2
            && input[0].eq_ascii(b'g')
            && matches!(input[1].to_ascii(), Some(b'i' | b'e' | b'y'))
        {
            out.push(U::from_ascii(b'j'));
            out.push(input[1]);
            i = 2;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
}

/// `/gg/g -> 'g'`
fn r_gg<U: Unit>(input: &[U], out: &mut Vec<U>) {
    replace_digraph(input, out, b"gg", b'g');
}

/// `/g/g -> 'k'`
fn r_g_to_k<U: Unit>(input: &[U], out: &mut Vec<U>) {
    replace_unit(input, out, b'g', b'k');
}

/// `/([aeiou])h([^aeiou]|$)/g -> '$1$2'`
fn r_drop_h<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if is_vowel(input[i]) && i + 1 < n && input[i + 1].eq_ascii(b'h') {
            if i + 2 < n && !is_vowel(input[i + 2]) {
                out.push(input[i]);
                out.push(input[i + 2]);
                i += 3;
                continue;
            }
            if i + 2 == n {
                out.push(input[i]);
                i += 2;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
}

/// `/ck/g -> 'k'`
fn r_ck<U: Unit>(input: &[U], out: &mut Vec<U>) {
    replace_digraph(input, out, b"ck", b'k');
}

/// `/ph/g -> 'f'`
fn r_ph<U: Unit>(input: &[U], out: &mut Vec<U>) {
    replace_digraph(input, out, b"ph", b'f');
}

/// `/q/g -> 'k'`
fn r_q<U: Unit>(input: &[U], out: &mut Vec<U>) {
    replace_unit(input, out, b'q', b'k');
}

/// `/s(h|io|ia)/g -> 'x$1'`
fn r_s_soft<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if input[i].eq_ascii(b's') {
            if i + 1 < n && input[i + 1].eq_ascii(b'h') {
                out.push(U::from_ascii(b'x'));
                out.push(input[i + 1]);
                i += 2;
                continue;
            }
            // Ordered alternation: `io` before `ia`. They are disjoint, so the
            // order is not observable here — but it costs nothing to keep.
            if i + 2 < n
                && input[i + 1].eq_ascii(b'i')
                && matches!(input[i + 2].to_ascii(), Some(b'o' | b'a'))
            {
                out.push(U::from_ascii(b'x'));
                out.push(input[i + 1]);
                out.push(input[i + 2]);
                i += 3;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
}

/// `/^x/ -> 's'`
fn r_initial_x<U: Unit>(input: &[U], out: &mut Vec<U>) {
    out.extend_from_slice(input);
    if out.first().is_some_and(|u| u.eq_ascii(b'x')) {
        out[0] = U::from_ascii(b's');
    }
}

/// `/x/g -> 'ks'`
fn r_x_to_ks<U: Unit>(input: &[U], out: &mut Vec<U>) {
    for &u in input {
        if u.eq_ascii(b'x') {
            out.push(U::from_ascii(b'k'));
            out.push(U::from_ascii(b's'));
        } else {
            out.push(u);
        }
    }
}

/// `/t(ia|io)/g -> 'x$1'`
fn r_tia<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if input[i].eq_ascii(b't')
            && i + 2 < n
            && input[i + 1].eq_ascii(b'i')
            && matches!(input[i + 2].to_ascii(), Some(b'a' | b'o'))
        {
            out.push(U::from_ascii(b'x'));
            out.push(input[i + 1]);
            out.push(input[i + 2]);
            i += 3;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
}

/// `/th/ -> '0'` — **not global**.
fn r_first_th<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    for i in 0..n {
        if input[i].eq_ascii(b't') && i + 1 < n && input[i + 1].eq_ascii(b'h') {
            out.extend_from_slice(&input[..i]);
            out.push(U::from_ascii(b'0'));
            out.extend_from_slice(&input[i + 2..]);
            return;
        }
    }
    out.extend_from_slice(input);
}

/// `/tch/g -> 'ch'`
fn r_tch<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if input[i].eq_ascii(b't')
            && i + 2 < n
            && input[i + 1].eq_ascii(b'c')
            && input[i + 2].eq_ascii(b'h')
        {
            out.push(input[i + 1]);
            out.push(input[i + 2]);
            i += 3;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
}

/// `/v/g -> 'f'`
fn r_v<U: Unit>(input: &[U], out: &mut Vec<U>) {
    replace_unit(input, out, b'v', b'f');
}

/// `/^wh/ -> 'w'`
fn r_wh<U: Unit>(input: &[U], out: &mut Vec<U>) {
    if input.len() >= 2 && input[0].eq_ascii(b'w') && input[1].eq_ascii(b'h') {
        out.push(input[0]);
        out.extend_from_slice(&input[2..]);
    } else {
        out.extend_from_slice(input);
    }
}

/// `/w([^aeiou]|$)/g -> '$1'`
fn r_drop_w<U: Unit>(input: &[U], out: &mut Vec<U>) {
    drop_before_consonant(input, out, b'w');
}

/// `/y([^aeiou]|$)/g -> '$1'`
fn r_drop_y<U: Unit>(input: &[U], out: &mut Vec<U>) {
    drop_before_consonant(input, out, b'y');
}

/// `/z/ -> 's'` — **not global**.
fn r_first_z<U: Unit>(input: &[U], out: &mut Vec<U>) {
    out.extend_from_slice(input);
    if let Some(i) = out.iter().position(|u| u.eq_ascii(b'z')) {
        out[i] = U::from_ascii(b's');
    }
}

/// `charAt(0) + substr(1).replace(/[aeiou]/g, '')`
fn r_drop_vowels<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let Some((&first, rest)) = input.split_first() else {
        return;
    };
    out.push(first);
    out.extend(rest.iter().copied().filter(|&u| !is_vowel(u)));
}

// -- fused rules, used only by `pipeline` -------------------------------------
//
// Each fusion is proved by the same argument the pre-existing `r_c_soft`
// fusion uses — no replacement creates work for a later fused sub-pass, and
// scanning stays left-to-right and non-overlapping — and each is pinned by an
// exhaustive differential test against its sequential sub-passes
// (`tests::parity`).

/// Stage 6 `dTransform` fused: `/d(ge|gy|gi)/g -> 'j$1'`, then `/d/g -> 't'`.
///
/// Sound because the first pass's replacement `j$1` contains no `d`, so the
/// second pass rewrites exactly the `d`s the first pass left alone — which is
/// what the `else` arm does directly.
fn r_d_fused<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if input[i].eq_ascii(b'd') {
            if i + 2 < n
                && input[i + 1].eq_ascii(b'g')
                && matches!(input[i + 2].to_ascii(), Some(b'e' | b'y' | b'i'))
            {
                out.push(U::from_ascii(b'j'));
                out.push(input[i + 1]);
                out.push(input[i + 2]);
                i += 3;
            } else {
                out.push(U::from_ascii(b't'));
                i += 1;
            }
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
}

/// Stage 8 `transformG` fused: `/gh/g -> 'f'`, `/([^g]|^)(g)(i|e|y)/g ->
/// '$1j$3'`, `/gg/g -> 'g'`, `/g/g -> 'k'`, in that pass order.
///
/// Processed per maximal `g`-run, which reproduces the four sequential passes
/// exactly:
///
/// * Pass 1 (`gh`→`f`) pairs left-to-right, so in a run of `m` `g`s followed
///   by `h` the **last** `g` absorbs the `h` into `f`; the run it leaves
///   behind has `m - 1` `g`s followed by `f`. Its output never contains `gh`
///   (a surviving `g` was, at scan time, followed by a non-`h`, and a
///   consumed pair leaves `f`), so passes 2–4 see a `gh`-free text.
/// * Pass 2 can only rewrite a **lone** `g` (a run of length 1 after pass 1):
///   in a longer run the first `g` is followed by `g` ∉ `iey` and every later
///   `g` has a `g` as its `[^g]` context. Its context unit must exist, not be
///   `g`, and not have been consumed as a previous match's `$3` vowel —
///   tracked here by `ctx_ok` over pass-1 output units (an emitted `f`
///   counts, as in `ghge` → `fje`). The `^` branch is `i == 0`, which is
///   pass-1-position 0 because pass 1 never removes the first unit.
/// * Passes 3+4 turn a run of `k` remaining `g`s into `⌈k/2⌉` `k`s: pass 3
///   pairs within the run (runs are separated by non-`g` units, so global
///   left-to-right pairing equals per-run pairing) and pass 4 maps the
///   survivors; no later pass consumes `g`, so both happen in place.
fn r_g_fused<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut i = 0;
    let mut ctx_ok = false;
    while i < n {
        if !input[i].eq_ascii(b'g') {
            out.push(input[i]);
            ctx_ok = true;
            i += 1;
            continue;
        }
        // The maximal run of `g`s, and whether pass 1 steals its last `g`.
        let mut m = 1;
        while i + m < n && input[i + m].eq_ascii(b'g') {
            m += 1;
        }
        let gh = i + m < n && input[i + m].eq_ascii(b'h');
        let run = m - usize::from(gh);
        if run == 0 {
            // A bare `gh`: pass 1 collapses it to `f`, an ordinary non-`g`
            // context unit for pass 2.
            out.push(U::from_ascii(b'f'));
            ctx_ok = true;
            i += 2;
            continue;
        }
        if run == 1
            && !gh
            && (i == 0 || ctx_ok)
            && i + 1 < n
            && matches!(input[i + 1].to_ascii(), Some(b'i' | b'e' | b'y'))
        {
            // Pass 2: a lone `g` before `i`/`e`/`y` with usable context.
            out.push(U::from_ascii(b'j'));
            out.push(input[i + 1]);
            // The vowel was consumed as `$3`: not context for the next `g`.
            ctx_ok = false;
            i += 2;
            continue;
        }
        // Passes 3+4 on the run's survivors.
        for _ in 0..run.div_ceil(2) {
            out.push(U::from_ascii(b'k'));
        }
        i += m;
        if gh {
            out.push(U::from_ascii(b'f'));
            ctx_ok = true;
            i += 1;
        } else {
            // The unit before whatever follows is a `g` in pass-2 terms; the
            // next loop iteration (a non-`g`) resets this anyway.
            ctx_ok = false;
        }
    }
}

/// Stage 13 `transformX` fused: `/^x/ -> 's'`, then `/x/g -> 'ks'`.
///
/// Sound because the first pass's replacement `s` is not `x`, so the second
/// pass rewrites exactly the non-initial `x`s.
fn r_x_fused<U: Unit>(input: &[U], out: &mut Vec<U>) {
    for (i, &u) in input.iter().enumerate() {
        if u.eq_ascii(b'x') {
            if i == 0 {
                out.push(U::from_ascii(b's'));
            } else {
                out.push(U::from_ascii(b'k'));
                out.push(U::from_ascii(b's'));
            }
        } else {
            out.push(u);
        }
    }
}

/// Stage 14 `transformT` fused: `/t(ia|io)/g -> 'x$1'`, then the non-global
/// `/th/ -> '0'`.
///
/// Sound because the first pass neither creates nor destroys a `th` (see
/// [`seek_t_fused`]) and preserves the relative order of the `t`s it leaves,
/// so "the first `th` in pass-1 output" is "the first unconsumed `t`+`h`
/// met by this scan" — the `th_done` flag. At a single `t` the two matches
/// are disjoint (`i` vs `h` next), so testing `t(ia|io)` first is the
/// sequential order without being observable.
fn r_t_fused<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let n = input.len();
    let mut th_done = false;
    let mut i = 0;
    while i < n {
        if input[i].eq_ascii(b't') {
            if i + 2 < n
                && input[i + 1].eq_ascii(b'i')
                && matches!(input[i + 2].to_ascii(), Some(b'a' | b'o'))
            {
                out.push(U::from_ascii(b'x'));
                out.push(input[i + 1]);
                out.push(input[i + 2]);
                i += 3;
                continue;
            }
            if !th_done && i + 1 < n && input[i + 1].eq_ascii(b'h') {
                out.push(U::from_ascii(b'0'));
                th_done = true;
                i += 2;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
}

/// Stages 11 + 16 + 20 + 21 in stage 21's single scan: `/q/g -> 'k'` (11),
/// `/v/g -> 'f'` (16), the non-global `/z/ -> 's'` (20), then `charAt(0) +
/// substr(1).replace(/[aeiou]/g, '')` (21).
///
/// Stages 11 and 16 commute to here — see [`pipeline`]'s table — and their
/// maps touch disjoint letters whose outputs are never `z` or vowels, so
/// applying all three maps in one pass preserves both "the first `z`" and
/// every vowel-drop decision. Position 0 is emitted after mapping, exactly as
/// the sequential pipeline maps first (11/16/20) and keeps `charAt(0)`
/// verbatim last (21).
fn r_final<U: Unit>(input: &[U], out: &mut Vec<U>) {
    let mut z_done = false;
    for (i, &u) in input.iter().enumerate() {
        let mapped = if u.eq_ascii(b'q') {
            U::from_ascii(b'k')
        } else if u.eq_ascii(b'v') {
            U::from_ascii(b'f')
        } else if !z_done && u.eq_ascii(b'z') {
            z_done = true;
            U::from_ascii(b's')
        } else {
            u
        };
        if i == 0 || !is_vowel(mapped) {
            out.push(mapped);
        }
    }
}

/// `/<c>([^aeiou]|$)/g -> '$1'`, shared by `dropW` and `dropY`.
fn drop_before_consonant<U: Unit>(input: &[U], out: &mut Vec<U>, c: u8) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if input[i].eq_ascii(c) {
            if i + 1 < n && !is_vowel(input[i + 1]) {
                out.push(input[i + 1]);
                i += 2;
                continue;
            }
            if i + 1 == n {
                i += 1;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
}

/// `/<from>/g -> '<to>'` for a single unit.
fn replace_unit<U: Unit>(input: &[U], out: &mut Vec<U>, from: u8, to: u8) {
    out.extend(input.iter().map(|&u| {
        if u.eq_ascii(from) {
            U::from_ascii(to)
        } else {
            u
        }
    }));
}

/// `/<from>/g -> '<to>'` for a two-unit literal, non-overlapping.
fn replace_digraph<U: Unit>(input: &[U], out: &mut Vec<U>, from: &[u8; 2], to: u8) {
    let n = input.len();
    let mut i = 0;
    while i < n {
        if input[i].eq_ascii(from[0]) && i + 1 < n && input[i + 1].eq_ascii(from[1]) {
            out.push(U::from_ascii(to));
            i += 2;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
}

impl verbora_core::Phonetic for Metaphone {
    fn process(&self, token: &str) -> String {
        Self::process(self, token)
    }

    fn compare(&self, a: &str, b: &str) -> bool {
        Self::compare(self, a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mp() -> Metaphone {
        Metaphone::new()
    }

    #[test]
    fn encodes_the_reference_vectors() {
        let m = mp();
        for (input, want) in [
            ("ablaze", "ABLS"),
            ("transition", "TRNSXN"),
            ("astronomical", "ASTRNMKL"),
            ("buzzard", "BSRT"),
            ("wonderer", "WNTRR"),
            ("district", "TSTRKT"),
            ("hockey", "HK"),
            ("capital", "KPTL"),
            ("penguin", "PNKN"),
            ("garbonzo", "KRBNS"),
            ("lightning", "LTNNK"),
            ("light", "LT"),
            ("phonetics", "FNTKS"),
            ("fonetix", "FNTKS"),
            ("caesar", "KSR"),
            ("pharaoh", "FR"),
            ("tough", "TF"),
            ("thumb", "0M"),
            ("through", "0RF"),
            ("knuth", "N0"),
            ("gnat", "NT"),
            ("aegis", "EJS"),
            ("pneumatic", "NMTK"),
            ("wrack", "RK"),
        ] {
            assert_eq!(m.process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn reproduces_the_known_ch_bug() {
        // transformX runs after cTransform, so a word-initial `ch` -> `xh`
        // becomes `sh`. The reference's own spec has this case commented out.
        assert_eq!(mp().process("chemical"), "SHMKL");
        assert_eq!(mp().process("ch"), "SH");
        assert_eq!(mp().process("chch"), "SHKH");
        assert_eq!(mp().process("schch"), "SKHKSH");
    }

    #[test]
    fn non_global_replaces_hit_only_the_first_match() {
        let m = mp();
        assert_eq!(m.transform_t("thth"), "0th");
        assert_eq!(m.transform_z("zzz"), "szz");
        assert_eq!(m.process("thth"), "0TH");
        assert_eq!(m.process("zzz"), "SZ");
        assert_eq!(m.process("zaz"), "SZ");
    }

    #[test]
    fn dedup_collapses_pairs_not_runs() {
        let m = mp();
        assert_eq!(m.dedup("aaa"), "aa");
        assert_eq!(m.dedup("aaaa"), "aa");
        assert_eq!(m.dedup("aaaaa"), "aaa");
        // The exemption is case-sensitive: `[^c]` has no /i flag.
        assert_eq!(m.dedup("CC"), "C");
        assert_eq!(m.dedup("cc"), "cc");
        assert_eq!(m.dedup("Cc"), "Cc");
        assert_eq!(m.dedup("cC"), "cC");
    }

    #[test]
    fn dedup_is_code_unit_based() {
        // The four code units of "😀😀" are D83D DE00 D83D DE00 — no two
        // adjacent ones are equal, so nothing collapses. A char-based port
        // would return a single emoji.
        assert_eq!(mp().dedup("😀😀"), "😀😀");
        assert_eq!(mp().dedup("𝐀𝐀"), "𝐀𝐀");
    }

    #[test]
    fn context_consuming_rules_skip_adjacent_matches() {
        let m = mp();
        assert_eq!(m.c_transform("chch"), "xhkh");
        assert_eq!(m.c_transform("schch"), "skhxh");
        assert_eq!(m.c_transform("chchch"), "xhkhxh");
        assert_eq!(m.c_transform("sch"), "skh");
        assert_eq!(m.transform_g("ggi"), "ki");
        assert_eq!(m.transform_g("gggi"), "kki");
        assert_eq!(m.drop_h("ohhoh"), "oho");
        assert_eq!(m.drop_h("ahah"), "aha");
        assert_eq!(m.drop_h("ahaha"), "ahaha");
        assert_eq!(m.drop_w("www"), "w");
        assert_eq!(m.drop_w("wwaw"), "wa");
        assert_eq!(m.drop_y("yyy"), "y");
        assert_eq!(m.transform_s("sshsh"), "sxhxh");
    }

    #[test]
    fn c_transform_trims_reference_whitespace() {
        // U+FEFF is whitespace to the reference's trim but not to Rust's.
        assert_eq!(mp().c_transform("\u{FEFF}abc\u{FEFF}"), "abk");
        assert_eq!(mp().c_transform("  hello  "), "hello");
    }

    #[test]
    fn drop_g_leaves_word_final_gh_alone() {
        let m = mp();
        assert_eq!(m.drop_g("gh"), "gh");
        assert_eq!(m.drop_g("ghx"), "hx");
        assert_eq!(m.drop_g("gha"), "gha");
        assert_eq!(m.drop_g("gn"), "n");
        assert_eq!(m.drop_g("gned"), "ned");
        assert_eq!(m.drop_g("gnedx"), "gnedx");
    }

    #[test]
    fn stage_vectors_from_the_reference_spec() {
        let m = mp();
        assert_eq!(m.drop_initial_letters("knuth"), "nuth");
        assert_eq!(m.drop_initial_letters("garbage"), "garbage");
        assert_eq!(m.drop_b_after_m_at_end("dumb"), "dum");
        assert_eq!(m.drop_b_after_m_at_end("dumbo"), "dumbo");
        assert_eq!(m.c_transform("change"), "xhange");
        assert_eq!(m.c_transform("discharger"), "diskharger");
        assert_eq!(m.c_transform("aesthetician"), "aesthetixian");
        assert_eq!(m.c_transform("cieling"), "sieling");
        assert_eq!(m.c_transform("cuss"), "kuss");
        assert_eq!(m.d_transform("abridge"), "abrijge");
        assert_eq!(m.d_transform("bid"), "bit");
        assert_eq!(m.drop_g("alight"), "aliht");
        assert_eq!(m.drop_g("aligned"), "alined");
        assert_eq!(m.transform_g("age"), "aje");
        assert_eq!(m.transform_g("gin"), "jin");
        assert_eq!(m.transform_g("august"), "aukust");
        assert_eq!(m.transform_g("aggrade"), "akrade");
        assert_eq!(m.drop_h("alriht"), "alrit");
        assert_eq!(m.transform_ck("check"), "chek");
        assert_eq!(m.transform_ph("phone"), "fone");
        assert_eq!(m.transform_q("quack"), "kuack");
        assert_eq!(m.transform_s("shack"), "xhack");
        assert_eq!(m.transform_t("dementia"), "demenxia");
        assert_eq!(m.transform_t("that"), "0at");
        assert_eq!(m.drop_t("backstitch"), "backstich");
        assert_eq!(m.transform_v("vestige"), "festige");
        assert_eq!(m.transform_wh("whisper"), "wisper");
        assert_eq!(m.transform_x("xenophile"), "senophile");
        assert_eq!(m.transform_x("admixed"), "admiksed");
        assert_eq!(m.drop_y("analyzer"), "analzer");
        assert_eq!(m.drop_y("specify"), "specif");
        assert_eq!(m.transform_z("blaze"), "blase");
        assert_eq!(m.drop_vowels("adamantium"), "admntm");
        assert_eq!(m.drop_vowels(""), "");
        assert_eq!(m.drop_vowels("a"), "a");
    }

    #[test]
    fn edge_inputs() {
        let m = mp();
        assert_eq!(m.process(""), "");
        assert_eq!(m.process("a"), "A");
        assert_eq!(m.process("x"), "S");
        assert_eq!(m.process("c"), "K");
        assert_eq!(m.process("aaa"), "A");
        assert_eq!(m.process("ccc"), "KKK");
        assert_eq!(m.process("1234"), "1234");
        assert_eq!(m.process("a-b"), "A-B");
        assert_eq!(m.process("it's"), "IT'S");
        assert_eq!(m.process("/path"), "/P0");
        assert_eq!(m.process("  spaces  "), "SPSS");
        assert_eq!(m.process("ch ch"), "SH KSH");
    }

    #[test]
    fn unicode_inputs() {
        let m = mp();
        assert_eq!(m.process("ß"), "SS");
        assert_eq!(m.process("ﬁ"), "FI");
        assert_eq!(m.process("ÉCOLE"), "ÉKL");
        assert_eq!(m.process("café"), "KFÉ");
        assert_eq!(m.process("naïve"), "NÏF");
        assert_eq!(m.process("😀"), "😀");
        assert_eq!(m.process("Москва"), "МОСКВА");
        assert_eq!(m.process("日本語"), "日本語");
    }

    #[test]
    fn truncation_happens_before_uppercasing() {
        let m = mp();
        assert_eq!(m.process_with("phonetics", Some(4.0)), "FNTK");
        assert_eq!(m.process_with("phonetics", Some(8.0)), "FNTKS");
        assert_eq!(m.process_with("phonetics", Some(1.0)), "F");
        assert_eq!(m.process_with("phonetics", Some(-1.0)), "");
        assert_eq!(m.process_with("phonetics", Some(0.0)), "FNTKS");
        assert_eq!(m.process_with("phonetics", Some(f64::NAN)), "FNTKS");
        // Three code units in, six characters out: `ß` uppercases to `SS`.
        assert_eq!(m.process_with(&"ß".repeat(40), Some(3.0)), "SSSSSS");
    }

    #[test]
    fn truncation_can_orphan_a_surrogate() {
        let m = mp();
        assert_eq!(m.process_utf16("😀", Some(1.0)), vec![0xD83D]);
        assert_eq!(m.process_with("😀", Some(1.0)), "\u{FFFD}");
    }

    #[test]
    fn very_long_input() {
        let m = mp();
        assert_eq!(m.process(&"a".repeat(500)), "A");
        assert_eq!(m.process(&"ab".repeat(300)).len(), 32);
    }

    #[test]
    fn compare_uses_the_default_length() {
        let m = mp();
        assert!(m.compare("phonetics", "fonetix"));
        assert!(!m.compare("PHONETICS", "garbonzo"));
    }

    /// Differential proof that the gated/fused [`pipeline`] is byte-identical
    /// to the stage-by-stage [`pipeline_reference`] oracle.
    mod parity {
        use super::super::*;

        /// Every observable `maxLength` regime: falsy (`None`/`0`/`NaN`),
        /// negative, truncating, and past-the-end.
        const MAX_LENGTHS: [Option<f64>; 8] = [
            None,
            Some(0.0),
            Some(-1.0),
            Some(1.0),
            Some(3.0),
            Some(4.0),
            Some(32.0),
            Some(f64::NAN),
        ];

        /// `Metaphone::process_with`, computed through the oracle pipeline.
        fn process_with_reference(token: &str, max_length: Option<f64>) -> String {
            let lower = utf16_to_lowercase(token);
            if lower.is_ascii() {
                let mut out = pipeline_reference(lower.as_bytes(), max_length);
                out.make_ascii_uppercase();
                String::from_utf8(out).expect("ASCII in, ASCII out")
            } else {
                let units: Vec<u16> = lower.encode_utf16().collect();
                let out = uppercase_utf16(&pipeline_reference(&units, max_length));
                String::from_utf16_lossy(&out)
            }
        }

        /// `Metaphone::process_utf16`, computed through the oracle pipeline.
        fn process_utf16_reference(token: &str, max_length: Option<f64>) -> Vec<u16> {
            use crate::units::to_utf16;
            let lower = utf16_to_lowercase(token);
            if lower.is_ascii() {
                let mut out = pipeline_reference(lower.as_bytes(), max_length);
                out.make_ascii_uppercase();
                to_utf16(&out)
            } else {
                let units: Vec<u16> = lower.encode_utf16().collect();
                uppercase_utf16(&pipeline_reference(&units, max_length))
            }
        }

        /// Asserts new == oracle for one token at one `maxLength`, on both
        /// public entry points.
        fn assert_parity_at(token: &str, max_length: Option<f64>) {
            let m = Metaphone::new();
            assert_eq!(
                m.process_with(token, max_length),
                process_with_reference(token, max_length),
                "process_with parity for {token:?} at {max_length:?}"
            );
            assert_eq!(
                m.process_utf16(token, max_length),
                process_utf16_reference(token, max_length),
                "process_utf16 parity for {token:?} at {max_length:?}"
            );
        }

        /// Asserts parity across every `maxLength` regime.
        fn assert_parity(token: &str) {
            for max_length in MAX_LENGTHS {
                assert_parity_at(token, max_length);
            }
        }

        /// xorshift64* — deterministic, dependency-free PRNG.
        struct Rng(u64);

        impl Rng {
            fn new(seed: u64) -> Self {
                Self(seed.max(1))
            }

            fn next(&mut self) -> u64 {
                let mut x = self.0;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                self.0 = x;
                x.wrapping_mul(0x2545_F491_4F6C_DD1D)
            }

            fn below(&mut self, n: usize) -> usize {
                (self.next() % n as u64) as usize
            }
        }

        /// Loads a workspace benchmark word list (`benches/data/<file>`).
        fn bench_list(file: &str, key: &str) -> Vec<String> {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .expect("crate is two levels below the workspace root")
                .join("benches/data")
                .join(file);
            let body = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
            json[key]
                .as_array()
                .expect("word array")
                .iter()
                .map(|w| w.as_str().expect("word is a string").to_owned())
                .collect()
        }

        /// Runs `f` over every string of length `0..=max_len` over `alphabet`.
        fn for_all_strings(alphabet: &[u8], max_len: u32, f: &mut impl FnMut(&[u8])) {
            let k = alphabet.len();
            let mut buf = Vec::with_capacity(max_len as usize);
            for len in 0..=max_len {
                for mut idx in 0..k.pow(len) {
                    buf.clear();
                    for _ in 0..len {
                        buf.push(alphabet[idx % k]);
                        idx /= k;
                    }
                    f(&buf);
                }
            }
        }

        /// A rewrite rule at byte width.
        type Rule8 = fn(&[u8], &mut Vec<u8>);

        /// One rule applied through [`Pipe`], at byte width.
        fn one8(input: &[u8], rule: Rule8) -> Vec<u8> {
            seq8(input, &[rule])
        }

        /// A rule sequence applied through [`Pipe`], at byte width.
        fn seq8(input: &[u8], rules: &[Rule8]) -> Vec<u8> {
            let mut p = Pipe::new(input);
            for &rule in rules {
                p.apply(rule);
            }
            p.finish()
        }

        #[test]
        fn differential_bench_names() {
            let names = bench_list("names.json", "names");
            assert!(names.len() >= 500, "bench name list unexpectedly small");
            for name in &names {
                assert_parity(name);
            }
        }

        #[test]
        fn differential_bench_words() {
            let words = bench_list("words.json", "words");
            assert!(words.len() >= 10_000, "bench word list unexpectedly small");
            for word in &words {
                for max_length in [None, Some(4.0), Some(-1.0)] {
                    assert_parity_at(word, max_length);
                }
            }
        }

        #[test]
        fn differential_existing_fixture_inputs() {
            for token in [
                "ablaze",
                "transition",
                "astronomical",
                "buzzard",
                "wonderer",
                "district",
                "hockey",
                "capital",
                "penguin",
                "garbonzo",
                "lightning",
                "light",
                "phonetics",
                "fonetix",
                "caesar",
                "pharaoh",
                "tough",
                "thumb",
                "through",
                "knuth",
                "gnat",
                "aegis",
                "pneumatic",
                "wrack",
                "chemical",
                "ch",
                "chch",
                "schch",
                "thth",
                "zzz",
                "zaz",
                "",
                "a",
                "x",
                "c",
                "aaa",
                "ccc",
                "1234",
                "a-b",
                "it's",
                "/path",
                "  spaces  ",
                "ch ch",
                "ß",
                "ﬁ",
                "ÉCOLE",
                "café",
                "naïve",
                "😀",
                "Москва",
                "日本語",
                "PHONETICS",
                "Kowalski",
            ] {
                assert_parity(token);
            }
            assert_parity(&"ß".repeat(40));
            assert_parity(&"a".repeat(500));
            assert_parity(&"ab".repeat(300));
        }

        /// Hand-built cases targeting each fusion and gate decision: inputs
        /// where one sub-pass's output creates (or destroys) a pattern the
        /// next sub-pass matches, overlap/priority collisions, anchored edits
        /// after the mid-pipeline trim, and mask-staleness cascades.
        #[test]
        fn differential_adversarial_fusion_cases() {
            const CASES: &[&str] = &[
                // d-fusion: dge creating j, d runs, dge/d overlap
                "ddge",
                "dgdge",
                "adgidgi",
                "ddd",
                "dgadge",
                "dged",
                "dg",
                "d",
                "dodge",
                "dodgy",
                "judge",
                "gadget",
                "dgia",
                "adgy",
                // x-fusion: initial x vs global x, x runs
                "xx",
                "xax",
                "axx",
                "xxx",
                "chx",
                "chxch",
                "xh",
                // t-fusion: first-th flag vs tia rewrites, adjacency
                "ththth",
                "tiath",
                "thtia",
                "tith",
                "titio",
                "tiotia",
                "th",
                "t",
                "tia",
                "ttia",
                "ttiath",
                "athth",
                "tio",
                "thio",
                "tithe",
                // g-fusion: gh priority over gg, run pairing, soft-g context
                // consumption, gned suffixes
                "ggh",
                "gghh",
                "ghgh",
                "ghghg",
                "gigi",
                "gigig",
                "egege",
                "egige",
                "gge",
                "ggge",
                "gggg",
                "ggggg",
                "gggi",
                "gghe",
                "gghi",
                "ghge",
                "gh",
                "g",
                "gggh",
                "ghhg",
                "igig",
                "gyge",
                "aggie",
                "aggi",
                "gng",
                "gn",
                "gned",
                "gnedg",
                "ngned",
                "ligned",
                "ghghi",
                "gighi",
                "gigh",
                "ghgi",
                "geggh",
                "gege",
                "xege",
                "gyg",
                "ghy",
                "yghy",
                // final-fusion: q/v/z maps commuting past wh/dropW/dropY,
                // first-z with vowel drops
                "zzz",
                "zaz",
                "az",
                "za",
                "qzv",
                "vqz",
                "aeiou",
                "aeiouz",
                "quiz",
                "viza",
                "zz",
                "v",
                "q",
                "z",
                "uvz",
                "qsh",
                "squio",
                "vw",
                "wv",
                "vwv",
                "qwq",
                "zwz",
                "wq",
                "whv",
                "whz",
                "wz",
                "yz",
                "zy",
                "qy",
                "vy",
                // anchored stages vs the mid-pipeline trim
                " kn",
                "kn ",
                " mb",
                "mb ",
                " wh",
                "wh ",
                "\u{FEFF}kn",
                " ch",
                "ch ",
                " x",
                "x ",
                "\u{FEFF}gh\u{FEFF}",
                "\u{FEFF}",
                "   ",
                " ",
                "a b",
                " a",
                "a ",
                "knmb",
                "wr",
                "ae",
                "kn",
                "mb",
                "amb",
                "whth",
                // mask/gate cascades: stages creating letters for later stages
                "cia",
                "acia",
                "ciacia",
                "cy",
                "circe",
                "cick",
                "ck",
                "cck",
                "sh",
                "si",
                "sio",
                "sia",
                "phph",
                "pph",
                "ohhoh",
                "ahaha",
                "www",
                "wwaw",
                "yyy",
                "sshsh",
                "backstitch",
                "tch",
                "tchtch",
            ];
            for token in CASES {
                assert_parity(token);
                // The same shapes through the UTF-16 pipeline monomorphization.
                assert_parity(&format!("é{token}"));
                assert_parity(&format!("{token}😀"));
            }
        }

        #[test]
        fn differential_random_ascii_alpha() {
            const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
            let mut rng = Rng::new(0x5EED_0001);
            for round in 0..6_000 {
                let len = rng.below(41);
                let token: String = (0..len)
                    .map(|_| char::from(LETTERS[rng.below(LETTERS.len())]))
                    .collect();
                let max_length = MAX_LENGTHS[round % MAX_LENGTHS.len()];
                assert_parity_at(&token, max_length);
            }
        }

        #[test]
        fn differential_random_full_domain() {
            const POOL: &[char] = &[
                'a', 'b', 'c', 'd', 'e', 'g', 'h', 'i', 'k', 'm', 'n', 'o', 'p', 'q', 's', 't',
                'u', 'v', 'w', 'x', 'y', 'z', 'A', 'C', 'G', 'H', 'K', 'S', 'T', 'W', 'Z', '0',
                '1', '9', ' ', '-', '\'', '/', '.', ',', ';', '!', '\t', '\n', '\u{FEFF}',
                '\u{00A0}', '\u{0085}', 'ß', 'ẞ', 'ﬁ', 'é', 'ç', 'ü', 'İ', 'ı', 'Σ', 'ς', 'σ', 'Ж',
                'м', 'а', '日', '語', '😀', '𝐀',
            ];
            let mut rng = Rng::new(0x5EED_0002);
            for round in 0..6_000 {
                let len = rng.below(41);
                let token: String = (0..len).map(|_| POOL[rng.below(POOL.len())]).collect();
                let max_length = MAX_LENGTHS[round % MAX_LENGTHS.len()];
                assert_parity_at(&token, max_length);
            }
        }

        /// Every string of length ≤ 4 over an alphabet stressing all 21
        /// stages, plus length ≤ 5 over a `g`/`d`-stage alphabet; whole
        /// pipeline, every `maxLength` regime.
        #[test]
        fn exhaustive_short_pipeline_sweep() {
            for_all_strings(b"acdeghiostwz", 4, &mut |bytes| {
                let token = std::str::from_utf8(bytes).expect("ASCII alphabet");
                assert_parity(token);
            });
            for_all_strings(b"ghiend", 5, &mut |bytes| {
                let token = std::str::from_utf8(bytes).expect("ASCII alphabet");
                assert_parity(token);
            });
        }

        /// Each fused rule against its sequential sub-passes, exhaustively
        /// over the alphabet that stresses that fusion's case analysis.
        #[test]
        fn fused_rules_match_their_sequential_sub_passes() {
            for_all_strings(b"dgeyia", 5, &mut |s| {
                assert_eq!(
                    one8(s, r_d_fused),
                    seq8(s, &[r_dge, r_d_to_t]),
                    "d-fusion diverges on {:?}",
                    String::from_utf8_lossy(s)
                );
            });
            for_all_strings(b"ghieyax", 6, &mut |s| {
                assert_eq!(
                    one8(s, r_g_fused),
                    seq8(s, &[r_gh_to_f, r_g_soft, r_gg, r_g_to_k]),
                    "g-fusion diverges on {:?}",
                    String::from_utf8_lossy(s)
                );
            });
            for_all_strings(b"xaks", 5, &mut |s| {
                assert_eq!(
                    one8(s, r_x_fused),
                    seq8(s, &[r_initial_x, r_x_to_ks]),
                    "x-fusion diverges on {:?}",
                    String::from_utf8_lossy(s)
                );
            });
            for_all_strings(b"tiaoh", 6, &mut |s| {
                assert_eq!(
                    one8(s, r_t_fused),
                    seq8(s, &[r_tia, r_first_th]),
                    "t-fusion diverges on {:?}",
                    String::from_utf8_lossy(s)
                );
            });
            for_all_strings(b"qvzaesk", 5, &mut |s| {
                assert_eq!(
                    one8(s, r_final),
                    seq8(s, &[r_q, r_v, r_first_z, r_drop_vowels]),
                    "final-fusion diverges on {:?}",
                    String::from_utf8_lossy(s)
                );
            });
        }

        /// The skip machinery itself: every seek (and every O(1) anchor gate)
        /// is an **exact** firing test — true ⟺ its rule changes the text —
        /// and the letter mask reports exactly the letters present.
        #[test]
        fn seeks_and_gates_are_exact_firing_tests() {
            type Case = (&'static str, fn(&[u8]) -> bool, Rule8);
            let cases: &[Case] = &[
                ("dedup", |s| scan_letters(s).1, r_dedup),
                (
                    "drop_initial",
                    |s| s.len() >= 2 && INITIAL_PREFIXES.iter().any(|p| eq_ascii_slice(&s[..2], p)),
                    r_drop_initial_letters,
                ),
                (
                    "mb$",
                    |s| s.len() >= 2 && eq_ascii_slice(&s[s.len() - 2..], b"mb"),
                    r_drop_b_after_m_at_end,
                ),
                ("ck", |s| seek_digraph(s, b'c', b'k'), r_ck),
                ("ch_to_xh", seek_ch_to_xh, r_ch_to_xh),
                ("trim", |s| trim_units(s).len() != s.len(), r_trim),
                ("c_soft", |s| seek_unit(s, b'c'), r_c_soft),
                ("d_fused", |s| seek_unit(s, b'd'), r_d_fused),
                (
                    "gh_before_consonant",
                    seek_gh_before_consonant,
                    r_gh_before_consonant,
                ),
                (
                    "drop_final_g",
                    |s| {
                        let n = s.len();
                        (n >= 2 && eq_ascii_slice(&s[n - 2..], b"gn"))
                            || (n >= 4 && eq_ascii_slice(&s[n - 4..], b"gned"))
                    },
                    r_drop_final_g,
                ),
                ("g_fused", |s| seek_unit(s, b'g'), r_g_fused),
                ("drop_h", seek_drop_h, r_drop_h),
                ("ph", |s| seek_digraph(s, b'p', b'h'), r_ph),
                ("s_soft", seek_s_soft, r_s_soft),
                ("x_fused", |s| seek_unit(s, b'x'), r_x_fused),
                ("t_fused", seek_t_fused, r_t_fused),
                ("tch", seek_tch, r_tch),
                (
                    "wh",
                    |s| s.len() >= 2 && s[0].eq_ascii(b'w') && s[1].eq_ascii(b'h'),
                    r_wh,
                ),
                ("drop_w", |s| seek_drop_before_consonant(s, b'w'), r_drop_w),
                ("drop_y", |s| seek_drop_before_consonant(s, b'y'), r_drop_y),
            ];

            let mut check = |s: &[u8]| {
                for (name, seek, rule) in cases {
                    let fired = one8(s, *rule) != s;
                    assert_eq!(
                        seek(s),
                        fired,
                        "seek {name} disagrees with its rule on {:?}",
                        String::from_utf8_lossy(s)
                    );
                }
                // `r_final`'s gate is conservative, not exact (a vowel at
                // position 0 alone passes the gate but changes nothing) — so
                // only the skip direction is load-bearing.
                let gate =
                    letter_mask(s) & (VOWEL_BITS | lbit(b'q') | lbit(b'v') | lbit(b'z')) != 0;
                if !gate {
                    assert_eq!(
                        one8(s, r_final),
                        s,
                        "r_final fired although its gate was off, on {:?}",
                        String::from_utf8_lossy(s)
                    );
                }
                // The mask is exact on the buffers `pipeline` computes it on.
                for b in b'a'..=b'z' {
                    assert_eq!(
                        letter_mask(s) & lbit(b) != 0,
                        s.contains(&b),
                        "letter mask wrong for {} on {:?}",
                        char::from(b),
                        String::from_utf8_lossy(s)
                    );
                }
            };

            for_all_strings(b"acdeghikmnopstwy", 4, &mut check);
            let mut rng = Rng::new(0x5EED_0003);
            const FULL: &[u8] = b"abcdefghijklmnopqrstuvwxyz 'h-";
            let mut buf = Vec::new();
            for _ in 0..3_000 {
                let len = rng.below(13);
                buf.clear();
                for _ in 0..len {
                    buf.push(FULL[rng.below(FULL.len())]);
                }
                check(&buf);
            }
        }

        /// Not a benchmark — a coarse stderr sanity signal that the gated
        /// pipeline is not slower than the staged oracle. Asserts parity only.
        #[test]
        fn rough_speed_sanity() {
            let names = bench_list("names.json", "names");
            let m = Metaphone::new();
            let mut new_len = 0usize;
            let mut old_len = 0usize;
            let mut new_time = std::time::Duration::MAX;
            let mut old_time = std::time::Duration::MAX;
            // Three interleaved rounds; keep each side's best, so CPU warmup
            // does not bias whichever side runs first.
            for _ in 0..3 {
                new_len = 0;
                let start = std::time::Instant::now();
                for _ in 0..50 {
                    for name in &names {
                        new_len += m.process(name).len();
                    }
                }
                new_time = new_time.min(start.elapsed());
                old_len = 0;
                let start = std::time::Instant::now();
                for _ in 0..50 {
                    for name in &names {
                        old_len += process_with_reference(name, None).len();
                    }
                }
                old_time = old_time.min(start.elapsed());
            }
            assert_eq!(new_len, old_len);
            eprintln!(
                "rough sanity (not a benchmark): gated pipeline {new_time:?} vs staged oracle {old_time:?} over {} encodes",
                names.len() * 50
            );
        }
    }
}
