//! The compatibility stamp a serialized corpus carries.
//!
//! The user-facing contract lives on [`ArtifactStamp`] and [`StampError`],
//! because this module is private and a private module's `//!` prose never
//! reaches the rendered documentation.

use std::sync::OnceLock;

/// The version of this crate's serialized shape and key-derivation pipeline.
///
/// Bump this by hand — in the same change — whenever either of the following
/// stops being true of artifacts written by older builds:
///
/// * the serialized shape is the one [`crate::TfIdf::from_json`] reads, or
/// * the terms in it are the ones this build's ingestion would produce from the
///   same document text and the same analyzer.
///
/// A bump refuses every older artifact, which is the point: the alternative is
/// loading one and answering queries against a term table that no longer
/// describes the corpus.
///
/// History, so a bump can be read rather than guessed at:
///
/// * `2` added the `lowercase` member, so the stamp covers the case fold in
///   front of the tokenizer as well as the tokenizer itself.
/// * `3` is the Rust-native contract: the serialized shape became
///   `{analyzer, documents}` with per-document term arrays, term counts became
///   integers, ingestion became *tokenize then fold* rather than *fold then
///   tokenize*, and the analyzer — case fold and stop-word list — became part
///   of the artifact.
pub const SCHEMA: u32 = 3;

/// The JSON member the stamp is written to and read from.
pub const STAMP_PROPERTY: &str = "_verbora";

/// A fingerprint of `str::to_lowercase`'s behaviour in this build.
///
/// The case fold is `std`'s, so it moves with the Rust toolchain and no
/// dependency version describes it; see [`ArtifactStamp`] for the measured
/// toolchain-to-toolchain difference that makes this a real hazard rather than
/// a theoretical one.
///
/// # Definition
///
/// FNV-1a, 64-bit (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`),
/// absorbed over this byte stream:
///
/// 1. for every Unicode scalar value from `U+0000` to `U+10FFFF` in ascending
///    order — surrogate code points are not scalar values and are skipped — the
///    UTF-8 bytes of that scalar's one-character string lowercased, then a
///    single `0xFF`;
/// 2. then, for each string in [`CONTEXT_PROBES`] in order, its lowercased
///    UTF-8 bytes, then a single `0xFF`.
///
/// `0xFF` cannot occur in UTF-8, so it separates the stream unambiguously and
/// the position of each field identifies the scalar it came from. The whole
/// definition is Verbora's: no hasher from a dependency is involved, because a
/// dependency bump must not be able to move this number.
///
/// Step 2 exists because `str::to_lowercase` is not `char::to_lowercase`
/// applied pointwise: `Σ` lowercases to `ς` or to `σ` depending on the
/// characters around it, and step 1 sees no context at all.
///
/// # Cost
///
/// One pass over 1,112,064 scalar values, measured in this crate at **21.5 ms**
/// under the workspace's `[profile.test]` (`opt-level = 2`) and **108 ms**
/// unoptimized. It is computed once per process behind a [`OnceLock`] and only
/// when something asks for a stamp, so it is paid by serialization and
/// deserialization, never by ingestion or query.
#[must_use]
pub fn lowercase_fingerprint() -> u64 {
    static FINGERPRINT: OnceLock<u64> = OnceLock::new();
    *FINGERPRINT.get_or_init(compute_lowercase_fingerprint)
}

/// The strings whose lowercase form depends on the characters around them.
///
/// `str::to_lowercase`'s one context-sensitive rule is Greek final sigma: `Σ`
/// becomes `ς` when it ends a word and `σ` otherwise. A pointwise walk over
/// single scalars can never reach the first branch, so these probes are
/// absorbed into [`lowercase_fingerprint`] after it. Everything else `std` does
/// specially — `İ` expanding to two scalars, for one — is a property of the
/// single scalar and is already covered by the walk.
pub const CONTEXT_PROBES: [&str; 6] = [
    "ΑΣ",   // word-final: ας
    "ΑΣΑ",  // medial: ασα
    "ΟΔΌΣ", // word-final after an accented vowel: οδός
    "ΑΣ'",  // followed by a case-ignorable, still word-final: ας'
    "Σ",    // no preceding cased character, so not final: σ
    "ΑΣΣ",  // only the last of a run is final: ασς
];

/// FNV-1a's 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a's 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// The field separator, which cannot occur in UTF-8.
const FIELD_SEPARATOR: u8 = 0xFF;

/// [`lowercase_fingerprint`]'s definition, evaluated.
fn compute_lowercase_fingerprint() -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    // One reused buffer: the walk is over a million scalars and the string does
    // not need to outlive its iteration.
    let mut scalar = String::with_capacity(4);
    for code_point in 0..=0x10_FFFF_u32 {
        let Some(c) = char::from_u32(code_point) else {
            continue;
        };
        scalar.clear();
        scalar.push(c);
        absorb(&mut hash, scalar.to_lowercase().as_bytes());
    }
    for probe in CONTEXT_PROBES {
        absorb(&mut hash, probe.to_lowercase().as_bytes());
    }
    hash
}

/// Absorbs one field — `bytes` then [`FIELD_SEPARATOR`] — into an FNV-1a state.
fn absorb(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes.iter().copied().chain([FIELD_SEPARATOR]) {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

/// Parses the sixteen lowercase hex digits a stamp carries.
///
/// Strict on both length and case: this build only ever writes the canonical
/// spelling, so anything else is damage rather than a version difference.
fn parse_fingerprint(s: &str) -> Option<u64> {
    if s.len() != 16
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    u64::from_str_radix(s, 16).ok()
}

/// The build that produced — or must produce — a serialized corpus.
///
/// # Why a serialized corpus needs a stamp at all
///
/// Every key in a serialized corpus is a **term**, and a term is the output of
/// the analyzer pipeline: segment the text at [UAX #29] word boundaries, keep
/// the segments containing a letter or a digit, case-fold each one, drop the
/// ones on the stop-word list. Two of those steps read Unicode tables that move
/// between releases. A corpus written by one build and restored by another does
/// not merely score differently — it is keyed on a partition of the text the
/// restoring build no longer produces, so queries miss silently: no error, no
/// warning, a plausible wrong number.
///
/// `docs/design/text-shaping-contract.md` §1 states the obligation this module
/// discharges: *any structure that persists tokenizer- or normalizer-derived
/// keys must stamp the Unicode version and refuse to load across a change*.
///
/// # The whole derivation, and where each part is recorded
///
/// A stamp that claimed more than it covers would be worse than none, because
/// it invites trust. So every input to a term is listed here, with the thing
/// that pins it:
///
/// | Input to a term | Pinned by |
/// |---|---|
/// | Word boundaries (UAX #29 `Word_Break`) | the Unicode version, below |
/// | The "contains a letter or digit" filter | the Unicode version, below |
/// | The case fold (`str::to_lowercase`) | [`lowercase_fingerprint`], below |
/// | Whether folding runs at all | recorded **in the artifact**, as the analyzer's `case_fold` |
/// | The stop-word list | recorded **in the artifact**, word for word |
/// | Which tokenizer ran | serialization is **refused** unless it was the default one |
/// | The serialized shape, and this table itself | [`SCHEMA`], below |
///
/// The last three rows are why this crate's coverage is now total rather than
/// partial. An earlier design left the stop-word list and a replaced tokenizer
/// outside the stamp and documented them as caller-owned risk; both are now
/// either written into the artifact or made impossible to write.
///
/// ## The three build facts
///
/// * [`SCHEMA`] — a Verbora-owned counter, bumped by hand whenever the
///   serialized shape or the key-derivation pipeline changes in a way that
///   makes an older artifact wrong. It is what covers a change no external
///   version number would show.
/// * The Unicode version, read at build time from
///   [`verbora_tokenizers::unicode_version`], which is whatever
///   `unicode-segmentation` ships and is pinned in `Cargo.lock`. The
///   `Word_Break` assignments the boundaries are computed from are that crate's
///   own tables. The alphanumeric property the word filter tests is *not*, or
///   not always: `unicode-segmentation` compares its own Unicode version
///   against the toolchain's `char::UNICODE_VERSION` and, when the two agree,
///   calls `std`'s `char::is_alphabetic` and `char::is_numeric` instead of
///   consulting its tables — falling back to them only when the versions
///   differ. Either branch therefore yields the property *as of the version
///   this field records*: the crate's tables are that version by construction,
///   and `std`'s are used only on the branch that has just checked they are the
///   same version. So one version number does cover the whole of step one — but
///   because of that equality check, not because the filter reads a dependency
///   table. Which branch a given build takes is observable: compare
///   [`verbora_tokenizers::unicode_version`] with `char::UNICODE_VERSION`. Under
///   the toolchain and lockfile this crate is developed against both report
///   17.0.0, so the delegating branch is the live one and no divergence exists
///   to be found.
/// * [`lowercase_fingerprint`] — a fingerprint of what `str::to_lowercase`
///   actually does in this build. That mapping is `std`'s, not a dependency's,
///   so it moves with the Rust toolchain and `Cargo.lock` records nothing about
///   it.
///
/// All three must match exactly. None is compared with an ordering: an artifact
/// from a *newer* build is refused just as one from an older build is, because
/// "newer" says nothing about whether the partition agrees.
///
/// The comparison is also unconditional. An artifact written with
/// [`CaseFold::None`](crate::CaseFold) is refused across a lowercase-mapping
/// change even though the mapping contributed nothing to a single one of its
/// keys. That refuses a few artifacts that would in fact have loaded correctly;
/// the alternative — making the comparison depend on the recorded analyzer —
/// makes the check itself analyzer-dependent, and a check that is sometimes
/// skipped is a check nobody can reason about.
///
/// ## Why the lowercase mapping needed a fact of its own
///
/// `str::to_lowercase` reads the case-mapping tables `std` bakes into
/// `core::unicode`, and those tables are regenerated whenever a Rust release
/// adopts a newer UCD. Nothing in `Cargo.lock` names the toolchain that
/// compiled the crate, so a stamp made of [`SCHEMA`] and
/// `unicode-segmentation`'s version records nothing about them: upgrading the
/// toolchain could re-key an entire corpus while leaving the stamp identical.
/// That is precisely the silent mismatch this module exists to prevent, reached
/// by the one route the stamp was not looking at.
///
/// It is not hypothetical. The same probe — lowercase every Unicode scalar
/// value and count the ones that do not map to themselves — compiled under two
/// toolchains:
///
/// | toolchain | scalars whose lowercase form is not themselves |
/// |---|---|
/// | 1.85.1 (2025-03-15) | 1460 |
/// | 1.97.1 (2026-07-14) | 1488 |
///
/// The 28 that differ are `U+A7CE`, `U+A7D2`, `U+A7D4`, and the 25 scalars
/// `U+16EA0..=U+16EB8`. Under the older toolchain each lowercases to itself;
/// under the newer one each lowercases to a different scalar (`U+A7CE` →
/// `U+A7CF`, `U+16EA0` → `U+16EBB`, and so on). A corpus containing any of
/// them, written by one and read by the other, is keyed on text the reading
/// build no longer produces — and [`verbora_tokenizers::unicode_version`] is
/// identical across the two, because it is a fact about a dependency rather
/// than about `std`.
///
/// The fingerprint is taken over *behaviour* rather than over a version number
/// for two reasons. `std` publishes no stable version number for these tables —
/// `char::UNICODE_VERSION` is permanently unstable — and a behavioural
/// fingerprint is the better test regardless: it changes exactly when a mapping
/// changes, and never merely because a version string was bumped.
///
/// # What "refuse" means, and how a user tells the two failures apart
///
/// A corrupt file and an incompatible file need opposite responses — repair or
/// re-fetch the first, re-index the second — so they are different errors.
/// [`StampError`] never reports a parse failure, and the parse failure
/// ([`crate::RestoreError::Parse`]) never reports a version problem.
///
/// Within [`StampError`] the three cases stay distinct for the same reason:
///
/// * [`StampError::Incompatible`] — a well-formed stamp naming a different
///   build. The artifact is intact and its provenance is known; it just cannot
///   be reused. Both stamps are carried so the message can name them.
/// * [`StampError::Malformed`] — a `_verbora` member that is not a stamp. This
///   is damage, not a version difference, and is the one `StampError` that
///   points at the file rather than at the build.
/// * [`StampError::Missing`] — no `_verbora` member at all.
///
/// # The unstamped artifact, which is the dangerous one
///
/// An artifact written before stamping existed is detectable **only as an
/// absence**. It is a well-formed JSON object with no `_verbora` member, and
/// nothing in it records which Unicode version produced its keys — that
/// information was never written down and cannot be recovered from the bytes.
/// It is therefore indistinguishable from a hand-written object, from one
/// produced by a different tool, and from one produced by a hypothetical future
/// build that dropped the stamp.
///
/// Because an unstamped artifact cannot be validated, it is **refused**, with
/// its own [`StampError::Missing`] variant rather than being folded into
/// `Incompatible`. Refusing is the only safe direction: accepting it would mean
/// guessing a version, and a wrong guess reproduces exactly the silent mismatch
/// the stamp exists to prevent. The distinct variant exists so the message can
/// say "rebuild this artifact" rather than "your file is damaged", which is the
/// response the user actually needs.
///
/// [UAX #29]: https://www.unicode.org/reports/tr29/
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactStamp {
    /// [`SCHEMA`] as of the build that wrote the artifact.
    pub schema: u32,
    /// The Unicode version whose `Word_Break` assignments and alphanumeric
    /// property produced the terms, as `(major, minor, update)`.
    pub unicode: (u64, u64, u64),
    /// [`lowercase_fingerprint`] as of the build that wrote the artifact.
    ///
    /// `None` for an artifact whose stamp carries no `lowercase` member, which
    /// every schema-1 artifact is: the member did not exist when they were
    /// written. Absence is a version difference rather than damage, so it is
    /// represented here instead of being rejected — [`StampError::Malformed`]
    /// stays reserved for a member that is present and wrong.
    pub lowercase: Option<u64>,
}

impl ArtifactStamp {
    /// The stamp this build writes, and the only one it accepts.
    ///
    /// The first call computes [`lowercase_fingerprint`]; every later one in
    /// the same process reads it back from a [`OnceLock`].
    #[must_use]
    pub fn current() -> Self {
        Self {
            schema: SCHEMA,
            unicode: verbora_tokenizers::unicode_version(),
            lowercase: Some(lowercase_fingerprint()),
        }
    }

    /// This stamp as the JSON object an artifact carries.
    pub(crate) fn to_json(self) -> serde_json::Value {
        let (major, minor, update) = self.unicode;
        let mut members = serde_json::Map::new();
        members.insert("schema".to_owned(), self.schema.into());
        members.insert(
            "unicode".to_owned(),
            format!("{major}.{minor}.{update}").into(),
        );
        if let Some(fingerprint) = self.lowercase {
            members.insert(
                "lowercase".to_owned(),
                format!("{fingerprint:016x}").as_str().into(),
            );
        }
        serde_json::Value::Object(members)
    }
}

impl std::fmt::Display for ArtifactStamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (major, minor, update) = self.unicode;
        write!(
            f,
            "schema {}, Unicode {major}.{minor}.{update}",
            self.schema
        )?;
        match self.lowercase {
            Some(fingerprint) => write!(f, ", lowercase {fingerprint:016x}"),
            None => f.write_str(", lowercase unrecorded"),
        }
    }
}

/// Why a serialized corpus was refused on its compatibility stamp.
///
/// This never reports a JSON syntax error: parsing failed or it did not, and
/// [`crate::RestoreError`] keeps the two apart so a caller can tell "this file
/// is damaged" from "this file is from another build".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StampError {
    /// The object carries no `_verbora` member.
    ///
    /// Either it predates stamping or it was not written by this library. In
    /// both cases the build behind its terms is unrecorded and unrecoverable,
    /// so it cannot be validated and is refused. Rebuild it from the source
    /// documents.
    Missing,
    /// The `_verbora` member is present but is not a stamp.
    ///
    /// The one variant that indicates a damaged file rather than a version
    /// difference. A stamp with no `lowercase` member is *not* this: that is
    /// what a schema-1 artifact looks like, and it is reported as
    /// [`Self::Incompatible`] with [`ArtifactStamp::lowercase`] set to `None`.
    Malformed,
    /// The stamp is well formed and names a different build.
    Incompatible {
        /// What the artifact says produced it.
        found: ArtifactStamp,
        /// What this build is.
        expected: ArtifactStamp,
    },
}

impl std::fmt::Display for StampError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(
                f,
                "serialized corpus carries no `{STAMP_PROPERTY}` compatibility stamp, \
                 so the build its terms were derived under is unknown; \
                 rebuild it with this version ({})",
                ArtifactStamp::current()
            ),
            Self::Malformed => write!(
                f,
                "serialized corpus has a `{STAMP_PROPERTY}` member that is not a \
                 compatibility stamp (expected {{\"schema\":<integer>,\"unicode\":\"<major>.<minor>.<update>\",\"lowercase\":\"<16 hex digits>\"}})"
            ),
            Self::Incompatible { found, expected } => write!(
                f,
                "serialized corpus was written by an incompatible build ({found}); \
                 this build is ({expected}). Its terms came out of a different \
                 segment-then-fold pipeline, so rebuild it from the source \
                 documents rather than loading it"
            ),
        }
    }
}

impl std::error::Error for StampError {}

/// Reads a non-negative integral schema number out of a JSON number.
fn read_schema(value: &serde_json::Value) -> Option<u32> {
    let number = value.as_number()?;
    if let Some(n) = number.as_u64() {
        return u32::try_from(n).ok();
    }
    // A writer that emitted `3.0` rather than `3` is spelling the same schema.
    let n = number.as_f64()?;
    if !n.is_finite() || n.fract() != 0.0 || n < 0.0 || n > f64::from(u32::MAX) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "range and integrality checked immediately above"
    )]
    Some(n as u32)
}

/// Parses `"major.minor.update"`, rejecting anything else.
fn parse_unicode(s: &str) -> Option<(u64, u64, u64)> {
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let update = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, update))
}

/// Reads the stamp out of a parsed corpus object.
///
/// # Errors
///
/// [`StampError::Missing`] when the object carries no `_verbora` member, and
/// [`StampError::Malformed`] when the member is present but is not a stamp.
pub fn read(value: &serde_json::Value) -> Result<ArtifactStamp, StampError> {
    let field = value.get(STAMP_PROPERTY).ok_or(StampError::Missing)?;
    if field.is_null() {
        return Err(StampError::Missing);
    }
    let (Some(schema), Some(unicode)) = (field.get("schema"), field.get("unicode")) else {
        return Err(StampError::Malformed);
    };
    let schema = read_schema(schema).ok_or(StampError::Malformed)?;
    let unicode = parse_unicode(unicode.as_str().ok_or(StampError::Malformed)?)
        .ok_or(StampError::Malformed)?;
    // Absent is a schema-1 stamp, which predates the member; present-and-wrong
    // is damage. The two get different answers.
    let lowercase = match field.get("lowercase") {
        None => None,
        Some(v) => Some(
            parse_fingerprint(v.as_str().ok_or(StampError::Malformed)?)
                .ok_or(StampError::Malformed)?,
        ),
    };
    Ok(ArtifactStamp {
        schema,
        unicode,
        lowercase,
    })
}

/// Accepts a parsed corpus object only if its stamp is this build's.
///
/// # Errors
///
/// [`StampError::Missing`], [`StampError::Malformed`] or
/// [`StampError::Incompatible`] — see each for what a caller should do about
/// it.
pub fn verify(value: &serde_json::Value) -> Result<(), StampError> {
    let found = read(value)?;
    let expected = ArtifactStamp::current();
    if found == expected {
        Ok(())
    } else {
        Err(StampError::Incompatible { found, expected })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The scalars whose `str::to_lowercase` image is known to differ between
    /// two real toolchains, as `(scalar, its lowercase form under 1.97.1)`.
    ///
    /// Under 1.85.1 — this workspace's MSRV — every one of them lowercases to
    /// itself. Both mappings are written out here so the tests below assert a
    /// difference between two *stated* mappings rather than between this build
    /// and a remembered one, which keeps them true on either toolchain and on a
    /// third that has neither.
    fn measured_divergences() -> Vec<(char, char)> {
        let mut out = vec![
            ('\u{A7CE}', '\u{A7CF}'),
            ('\u{A7D2}', '\u{A7D3}'),
            ('\u{A7D4}', '\u{A7D5}'),
        ];
        for i in 0..25_u32 {
            let upper = char::from_u32(0x1_6EA0 + i).expect("in range and not a surrogate");
            let lower = char::from_u32(0x1_6EBB + i).expect("in range and not a surrogate");
            out.push((upper, lower));
        }
        out
    }

    /// [`lowercase_fingerprint`]'s definition, transcribed from its doc comment
    /// rather than called, with `lower` supplying each scalar's image.
    ///
    /// The FNV-1a constants, the ascending scalar order, the `0xFF` field
    /// separator and the trailing [`CONTEXT_PROBES`] pass are all written out
    /// again here: if the implementation drifts from the documented definition,
    /// this disagrees with it.
    fn oracle_fingerprint(mut lower: impl FnMut(char) -> String) -> u64 {
        fn feed(hash: &mut u64, bytes: &[u8]) {
            for byte in bytes.iter().copied().chain([0xFF_u8]) {
                *hash ^= u64::from(byte);
                *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for code_point in 0..=0x10_FFFF_u32 {
            let Some(c) = char::from_u32(code_point) else {
                continue;
            };
            feed(&mut hash, lower(c).as_bytes());
        }
        for probe in CONTEXT_PROBES {
            feed(&mut hash, probe.to_lowercase().as_bytes());
        }
        hash
    }

    /// What `str::to_lowercase` does to a one-scalar string in this build.
    fn real_lowercase(c: char) -> String {
        c.to_string().to_lowercase()
    }

    /// A corpus object carrying a stamp, with `lowercase` present exactly when
    /// `lowercase` is.
    fn stamped_with(
        schema: serde_json::Value,
        unicode: &str,
        lowercase: Option<&str>,
    ) -> serde_json::Value {
        let mut stamp = serde_json::Map::new();
        stamp.insert("schema".to_owned(), schema);
        stamp.insert("unicode".to_owned(), unicode.into());
        if let Some(lowercase) = lowercase {
            stamp.insert("lowercase".to_owned(), lowercase.into());
        }
        json!({ STAMP_PROPERTY: serde_json::Value::Object(stamp) })
    }

    /// A corpus object carrying this build's own lowercase fingerprint, so the
    /// only things under test are `schema` and `unicode`.
    fn stamped(schema: serde_json::Value, unicode: &str) -> serde_json::Value {
        stamped_with(
            schema,
            unicode,
            Some(&format!("{:016x}", lowercase_fingerprint())),
        )
    }

    /// This build's Unicode version, spelled as the stamp writes it.
    fn current_version() -> String {
        let (major, minor, update) = ArtifactStamp::current().unicode;
        format!("{major}.{minor}.{update}")
    }

    // --- The fingerprint ---------------------------------------------------

    #[test]
    fn the_fingerprint_is_the_documented_walk_of_this_builds_own_mapping() {
        assert_eq!(lowercase_fingerprint(), oracle_fingerprint(real_lowercase));
    }

    #[test]
    fn the_fingerprint_is_stable_within_a_process() {
        // It is cached in a `OnceLock`, and a stamp that changed between two
        // calls would refuse artifacts this very build wrote.
        assert_eq!(lowercase_fingerprint(), lowercase_fingerprint());
        assert_eq!(
            ArtifactStamp::current().lowercase,
            Some(lowercase_fingerprint())
        );
    }

    #[test]
    fn the_two_measured_toolchains_would_not_share_a_fingerprint() {
        // The whole reason this member exists. `verbora_tokenizers::unicode_version`
        // is identical under both toolchains, because it describes
        // `unicode-segmentation` rather than `std`, so before this member a
        // corpus written under one and read under the other was accepted.
        let divergences = measured_divergences();
        assert_eq!(divergences.len(), 28, "the measured set is 3 + 25 scalars");

        // 1.85.1: each of them lowercases to itself.
        let older = oracle_fingerprint(|c| {
            if divergences.iter().any(|(upper, _)| *upper == c) {
                c.to_string()
            } else {
                real_lowercase(c)
            }
        });
        // 1.97.1: each maps to a distinct other scalar.
        let newer =
            oracle_fingerprint(
                |c| match divergences.iter().find(|(upper, _)| *upper == c) {
                    Some((_, lower)) => lower.to_string(),
                    None => real_lowercase(c),
                },
            );

        assert_ne!(
            older, newer,
            "the fingerprint must separate the two mappings it was built to separate"
        );
        // …and this build publishes whichever of them its own `std` has.
        assert!(
            lowercase_fingerprint() == older || lowercase_fingerprint() == newer,
            "this build's mapping agrees with one of the two outside the measured set"
        );
    }

    #[test]
    fn every_watched_scalars_mapping_reaches_the_fingerprint() {
        // Enumerated, not sampled: each scalar below is substituted for an
        // image no real mapping can produce (`char::to_lowercase` never yields
        // two NULs), and the fingerprint must move for every one of them.
        let mut watched: Vec<char> = vec![
            '\0',
            'A',
            'a',
            'Z',
            'İ',
            'Σ',
            'ς',
            'ẞ',
            'Ⅷ',
            'Ⓐ',
            '\u{10400}',
            '\u{1E900}',
            char::MAX,
        ];
        watched.extend(measured_divergences().iter().map(|(upper, _)| *upper));

        let real = lowercase_fingerprint();
        for c in watched {
            let perturbed = oracle_fingerprint(|x| {
                if x == c {
                    "\0\0".to_owned()
                } else {
                    real_lowercase(x)
                }
            });
            assert_ne!(
                perturbed, real,
                "U+{:04X} does not reach the fingerprint",
                c as u32
            );
        }
    }

    #[test]
    fn the_context_probes_pin_the_rule_a_scalar_walk_cannot_see() {
        // `str::to_lowercase` is not `char::to_lowercase` applied pointwise:
        // Greek final sigma depends on the characters around it. These are the
        // expected forms under UAX #21's `Final_Sigma` condition — a cased
        // character before, none after, ignoring case-ignorable characters.
        assert_eq!(
            CONTEXT_PROBES.map(str::to_lowercase),
            ["ας", "ασα", "οδός", "ας'", "σ", "ασς"].map(str::to_owned)
        );

        // Four of the six differ from the pointwise walk, which is what makes
        // them worth absorbing separately; the other two pin that the rule does
        // *not* fire where it must not.
        let pointwise: Vec<String> = CONTEXT_PROBES
            .iter()
            .map(|p| p.chars().flat_map(char::to_lowercase).collect())
            .collect();
        let differing = CONTEXT_PROBES
            .iter()
            .zip(&pointwise)
            .filter(|(probe, flat)| probe.to_lowercase() != **flat)
            .count();
        assert_eq!(differing, 4);
    }

    // --- Reading and refusing ----------------------------------------------

    #[test]
    fn the_current_stamp_round_trips_through_its_own_json() {
        let written = ArtifactStamp::current().to_json();
        assert!(written.get("lowercase").is_some(), "{written}");
        let corpus = json!({ STAMP_PROPERTY: written, "documents": [] });
        assert_eq!(verify(&corpus), Ok(()));
        assert_eq!(read(&corpus), Ok(ArtifactStamp::current()));
    }

    #[test]
    fn a_stamp_naming_a_foreign_lowercase_mapping_is_refused() {
        // The terms in a serialized corpus are cut out of text that
        // `str::to_lowercase` then folds, so a stamp naming a different mapping
        // describes different keys even when the schema and the tokenizer's
        // Unicode version agree.
        let foreign = format!("{:016x}", lowercase_fingerprint() ^ 1);
        let Err(StampError::Incompatible { found, expected }) = verify(&stamped_with(
            SCHEMA.into(),
            &current_version(),
            Some(&foreign),
        )) else {
            panic!("a foreign lowercase mapping must be refused");
        };
        assert_eq!(found.lowercase, Some(lowercase_fingerprint() ^ 1));
        assert_eq!(found.schema, expected.schema);
        assert_eq!(found.unicode, expected.unicode);
        assert_eq!(expected, ArtifactStamp::current());
    }

    #[test]
    fn a_stamp_with_no_lowercase_member_is_incompatible_not_malformed() {
        // What every schema-1 artifact looks like. It is intact and its
        // provenance is partly known, so it is a version difference — the
        // response is "rebuild it", not "your file is damaged".
        let Err(StampError::Incompatible { found, expected }) =
            verify(&stamped_with(SCHEMA.into(), &current_version(), None))
        else {
            panic!("a stamp with no lowercase member must be Incompatible");
        };
        assert_eq!(found.lowercase, None);
        assert_eq!(expected, ArtifactStamp::current());
        assert!(
            found.to_string().ends_with("lowercase unrecorded"),
            "{found}"
        );
    }

    #[test]
    fn an_absent_or_null_member_is_missing_not_malformed() {
        assert_eq!(
            verify(&json!({ "documents": [] })),
            Err(StampError::Missing)
        );
        assert_eq!(
            verify(&json!({ STAMP_PROPERTY: serde_json::Value::Null })),
            Err(StampError::Missing)
        );
        // A non-object corpus has no members at all.
        assert_eq!(verify(&json!([])), Err(StampError::Missing));
        assert_eq!(verify(&json!(7)), Err(StampError::Missing));
    }

    #[test]
    fn a_member_that_is_not_a_stamp_is_malformed() {
        let version = current_version();
        let fingerprint = format!("{:016x}", lowercase_fingerprint());
        for bad in [
            // Not an object at all.
            json!({ STAMP_PROPERTY: "17.0.0" }),
            json!({ STAMP_PROPERTY: 3 }),
            json!({ STAMP_PROPERTY: [] }),
            // Unicode version in the wrong shape.
            stamped(SCHEMA.into(), "17.0"),
            stamped(SCHEMA.into(), "17.0.0.0"),
            stamped(SCHEMA.into(), "seventeen"),
            stamped(SCHEMA.into(), ""),
            // Schema that is not a whole number in range.
            stamped(json!(1.5), "17.0.0"),
            stamped(json!(-1), "17.0.0"),
            stamped(json!("3"), "17.0.0"),
            stamped(json!(null), "17.0.0"),
            // A `lowercase` member that is present and is not sixteen lowercase
            // hex digits. Present-and-wrong is damage; absent is a version
            // difference, and has its own test above.
            stamped_with(SCHEMA.into(), &version, Some("")),
            stamped_with(SCHEMA.into(), &version, Some("0")),
            stamped_with(SCHEMA.into(), &version, Some(&fingerprint[..15])),
            stamped_with(SCHEMA.into(), &version, Some(&format!("0{fingerprint}"))),
            stamped_with(SCHEMA.into(), &version, Some("0123456789ABCDEF")),
            stamped_with(SCHEMA.into(), &version, Some("0x0123456789abcd")),
            stamped_with(SCHEMA.into(), &version, Some("0123456789abcdeg")),
            stamped_with(SCHEMA.into(), &version, Some(" 123456789abcdef")),
            // …and a `lowercase` member that is not a string at all.
            json!({ STAMP_PROPERTY: {
                "schema": SCHEMA,
                "unicode": version.clone(),
                "lowercase": 1,
            }}),
        ] {
            assert_eq!(verify(&bad), Err(StampError::Malformed), "{bad}");
        }
        // …and the shape that differs from the above only by being valid.
        assert_eq!(verify(&stamped(SCHEMA.into(), &version)), Ok(()));
        // A schema written as a float is the same schema.
        assert_eq!(verify(&stamped(json!(3.0), &version)), Ok(()));
    }

    #[test]
    fn a_foreign_schema_unicode_version_or_mapping_is_incompatible_in_both_directions() {
        let current = ArtifactStamp::current();
        let (major, minor, update) = current.unicode;
        let version = current_version();

        for schema in [current.schema - 1, current.schema + 1] {
            let Err(StampError::Incompatible { found, expected }) =
                verify(&stamped(schema.into(), &version))
            else {
                panic!("schema {schema} must be refused");
            };
            assert_eq!(found.schema, schema);
            assert_eq!(expected, current);
        }

        for other in [
            format!("{}.{minor}.{update}", major - 1),
            format!("{}.{minor}.{update}", major + 1),
            format!("{major}.{}.{update}", minor + 1),
            format!("{major}.{minor}.{}", update + 1),
        ] {
            assert!(
                matches!(
                    verify(&stamped(current.schema.into(), &other)),
                    Err(StampError::Incompatible { .. })
                ),
                "Unicode {other} must be refused"
            );
        }

        // Neither is the fingerprint ordered: one bit either way is refused,
        // and so is a mapping that is nobody's.
        let fingerprint = lowercase_fingerprint();
        for other in [
            fingerprint.wrapping_sub(1),
            fingerprint.wrapping_add(1),
            0,
            u64::MAX,
        ] {
            if other == fingerprint {
                continue;
            }
            assert!(
                matches!(
                    verify(&stamped_with(
                        current.schema.into(),
                        &version,
                        Some(&format!("{other:016x}"))
                    )),
                    Err(StampError::Incompatible { .. })
                ),
                "lowercase {other:016x} must be refused"
            );
        }
    }

    #[test]
    fn the_display_form_names_all_three_facts() {
        let (major, minor, update) = ArtifactStamp::current().unicode;
        assert_eq!(
            ArtifactStamp::current().to_string(),
            format!(
                "schema {SCHEMA}, Unicode {major}.{minor}.{update}, lowercase {:016x}",
                lowercase_fingerprint()
            )
        );
    }
}
