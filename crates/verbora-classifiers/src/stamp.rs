use std::sync::OnceLock;

use crate::dynval::DynValue;
use crate::stemmer::Stemmer;

/// The version of this crate's saved shape and feature-derivation pipeline.
///
/// Bump this by hand — in the same change — whenever either of the following
/// stops being true of models saved by older builds:
///
/// * the saved shape is the one [`Classifier::restore`](crate::Classifier::restore)
///   reads, or
/// * the feature keys in it are the ones this build would derive from the same
///   document text.
///
/// A bump refuses every older model, which is the point: the alternative is
/// loading one and classifying against a feature index that no longer describes
/// the vocabulary.
///
/// `2` added the `lowercase` member, so the stamp covers the case fold in front
/// of the tokenizer as well as the tokenizer itself. `3` added the `stemmer`
/// member, so it covers the rules that turn a token into a feature key; see the
/// [`ArtifactStamp`].
pub const SCHEMA: u32 = 3;

/// The JSON member the stamp is written to and read from.
pub const STAMP_PROPERTY: &str = "_verbora";

/// A fingerprint of `str::to_lowercase`'s behaviour in this build.
///
/// The case fold in front of the tokenizer is `std`'s, so it moves with the
/// Rust toolchain and no dependency version describes it; see the [`ArtifactStamp`]
/// for the measured toolchain-to-toolchain difference
/// that makes this a real hazard rather than a theoretical one.
///
/// # Definition
///
/// FNV-1a, 64-bit (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`),
/// absorbed over this byte stream:
///
/// 1. for every Unicode scalar value from `U+0000` to `U+10FFFF` in ascending
///    order — surrogate code points are not scalar values and are skipped —
///    the UTF-8 bytes of that scalar's one-character string lowercased, then a
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
/// It is defined identically in `verbora-tfidf`, and deliberately duplicated
/// for the reason the two `SCHEMA` counters are — see the [`ArtifactStamp`]'s closing section.
///
/// # Cost
///
/// One pass over 1,112,064 scalar values, measured at **21.5 ms** under the
/// workspace's `[profile.test]` (`opt-level = 2`) and **108 ms** unoptimized.
/// It is computed once per process behind a [`OnceLock`] and only when
/// something asks for a stamp, so it is paid by saving and loading, never by
/// training or classification.
#[must_use]
pub fn lowercase_fingerprint() -> u64 {
    static FINGERPRINT: OnceLock<u64> = OnceLock::new();
    *FINGERPRINT.get_or_init(compute_lowercase_fingerprint)
}

/// The documents [`stemmer_fingerprint`] hands every stemmer.
///
/// Chosen so that the sixteen stemmers `verbora-stemmers` publishes disagree
/// about them — Latin with and without diacritics, Cyrillic, Greek, Japanese,
/// Persian, and English words carrying every family of Porter suffix — plus
/// two shapes that are about the *tokenizer* rather than the rules: a
/// hyphenated compound and a decimal number. Coverage is not asserted from this
/// list; `every_bridged_stemmer_has_its_own_fingerprint` enumerates the shipped
/// stemmers and proves the sixteen fingerprints are pairwise distinct.
pub const STEMMER_PROBES: [&str; 12] = [
    "the running dogs jumped over relational happiness quickly",
    "conditional formalize electricity sensitivity hopefulness adjustable",
    "les chiens chantaient doucement pendant la nuit entière",
    "die Häuser wurden gestern Abend vollständig renoviert",
    "los niños corrían rápidamente hacia la montaña nevada",
    "as crianças brincavam alegremente no jardim florido",
    "de honden renden snel door het bos heen",
    "i bambini correvano velocemente verso la montagna",
    "быстрые коричневые лисицы прыгают через ленивых собак",
    "барвисті українські міста зустрічають гостей щоранку",
    "日本語のテキストを解析します",
    "unit-tests 3.14 İD ΟΔΌΣ naïve",
];

/// A fingerprint of what `stemmer` does to [`STEMMER_PROBES`].
///
/// # Definition
///
/// FNV-1a, 64-bit, with the same constants and the same `0xFF` field separator
/// as [`lowercase_fingerprint`], absorbed over this byte stream: for each probe
/// in [`STEMMER_PROBES`] in order, the UTF-8 bytes of each token of
/// `stemmer.tokenize_and_stem(probe, true)` in order, each followed by `0xFF`,
/// and then a single `0xFE` closing the probe. The extra separator is what
/// makes the token *count* part of the fingerprint: without it a stemmer that
/// merged two tokens into one would absorb the same bytes as one that did not.
///
/// `keep_stops` is `true` on purpose. The stop-word list is process-global
/// mutable state, not a fact about the build, and a fingerprint that moved when
/// a caller edited it would refuse models the same process had just saved. See
/// the [`ArtifactStamp`]'s "What the stamp deliberately does not
/// cover".
///
/// # Cost
///
/// Twelve short documents, stemmed once. It is paid by saving and loading,
/// never by training or classification, and is not cached: it is a function of
/// its argument, and the argument is the caller's.
#[must_use]
pub fn stemmer_fingerprint(stemmer: &(impl Stemmer + ?Sized)) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for probe in STEMMER_PROBES {
        for token in stemmer.tokenize_and_stem(probe, true) {
            absorb(&mut hash, token.as_bytes());
        }
        hash ^= u64::from(PROBE_SEPARATOR);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// The strings whose lowercase form depends on the characters around them.
///
/// `str::to_lowercase`'s one context-sensitive rule is Greek final sigma:
/// `Σ` becomes `ς` when it ends a word and `σ` otherwise. A pointwise walk over
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
/// The probe separator, likewise impossible in UTF-8, so a token boundary and a
/// document boundary can never be confused.
const PROBE_SEPARATOR: u8 = 0xFE;

/// [`lowercase_fingerprint`]'s definition, evaluated.
fn compute_lowercase_fingerprint() -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    // One reused buffer for the input: the walk is over a million scalars and
    // the string does not need to outlive its iteration.
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

/// The build that produced — or must produce — a saved model.
///
/// The compatibility stamp a saved model carries, and the reasons it can be
/// refused.
///
/// # Why a saved model needs a stamp at all
///
/// A [`Classifier`](crate::Classifier)'s feature keys are *stems of UAX #29
/// word tokens*: `add_document` hands the text to a
/// [`Stemmer`](crate::Stemmer), which case-folds it, tokenizes it, and stems
/// each token. Word boundaries are defined by UAX #29 over the `Word_Break`
/// property of the Unicode Character Database, so they move when the UCD moves
/// — and the case fold in front of them moves with the Rust toolchain, because
/// every stemmer in the workspace folds with `std`'s `str::to_lowercase`. When
/// either moves, the same document yields a different feature set. A model
/// trained before such a change and loaded after it is not merely less
/// accurate: its `features` map is keyed on a partition of the text the loading
/// build no longer produces, so probes miss, weights land on the wrong slots,
/// and the classifier returns a confident wrong label. There is no error and no
/// warning; the arithmetic is all valid.
///
/// The feature *index* makes it worse rather than better. Feature ids are
/// positions in an insertion-ordered map ([`OrderedMap`](crate::OrderedMap)), so a single extra
/// or missing feature shifts every id after it, and a trained weight vector
/// restored against a shifted index is scrambled rather than merely stale.
///
/// `docs/design/text-shaping-contract.md` §1 states the obligation this module
/// discharges: *any structure that persists tokenizer- or normalizer-derived
/// keys must stamp the Unicode version and refuse to load across a change*.
///
/// # What the stamp covers, and why it is four facts
///
/// The UCD version alone is not enough. Verbora's own segmentation contract can
/// change without a UCD bump — the tokenizer behind these stems was replaced
/// wholesale under a single Unicode version, moving `"unit-tests"` from one
/// feature to two — and the serialized *shape* can change without either.
/// Neither is the pair of those two enough, because the pipeline case-folds
/// before it tokenizes and neither of them watches that step. Nor is the
/// triple of those three enough, because a feature key is the *stemmer's*
/// output and none of them says which stemmer. So the stamp is a quadruple,
/// one fact per thing that can move independently:
///
/// * [`SCHEMA`] — a Verbora-owned counter, bumped by hand whenever the saved
///   shape or the feature-derivation pipeline changes in a way that makes an
///   older model wrong. It is what covers a change no external version number
///   would show.
/// * The Unicode version, read at build time from
///   [`verbora_tokenizers::unicode_version`], which is whatever
///   `unicode-segmentation` ships and is pinned in `Cargo.lock`. It covers the
///   `Word_Break` assignments the tokenizer cuts on.
/// * [`lowercase_fingerprint`] — a fingerprint of what `str::to_lowercase`
///   actually does in this build. That mapping is `std`'s, not a dependency's,
///   so it moves with the Rust toolchain and `Cargo.lock` records nothing
///   about it.
/// * [`stemmer_fingerprint`] — a fingerprint of what the classifier's own
///   [`Stemmer`](crate::Stemmer) does to a fixed probe corpus. The feature keys
///   *are* its output, and none of the other three describes it: two models
///   built by one binary from one corpus have nothing in common if one was
///   given `PorterStemmerFr` and the other the default.
///
/// All four must match exactly, and none is compared with an ordering: a model
/// from a *newer* build is refused just as one from an older build is, because
/// "newer" says nothing about whether the partition agrees.
///
/// ## Why the lowercase mapping needed a fact of its own
///
/// `str::to_lowercase` reads the case-mapping tables `std` bakes into
/// `core::unicode`, and those tables are regenerated whenever a Rust release
/// adopts a newer UCD. Nothing in `Cargo.lock` names the toolchain that
/// compiled the crate, so a stamp made of `SCHEMA` and
/// `unicode-segmentation`'s version records nothing about them: upgrading the
/// toolchain could re-key an entire model while leaving the stamp identical.
///
/// The fold is not incidental — it is *in front of* the tokenizer, not after
/// it. The default stemmer lowercases the whole document and the tokenizer then
/// cuts the result, so a changed mapping can move a word boundary as well as a
/// spelling.
///
/// It is not hypothetical either. The same probe — lowercase every Unicode
/// scalar value and count the ones that do not map to themselves — compiled
/// under two toolchains:
///
/// | toolchain | scalars whose lowercase form is not themselves |
/// |---|---|
/// | 1.85.1 (2025-03-15) | 1460 |
/// | 1.97.1 (2026-07-14) | 1488 |
///
/// The 28 that differ are `U+A7CE`, `U+A7D2`, `U+A7D4`, and the 25 scalars
/// `U+16EA0..=U+16EB8`. Under the older toolchain each lowercases to itself;
/// under the newer one each lowercases to a different scalar. A model trained
/// by one and loaded by the other is keyed on text the loading build no longer
/// produces — and [`verbora_tokenizers::unicode_version`] is identical across
/// the two, because it is a fact about a dependency rather than about `std`.
///
/// ## Why the stemmer needed a fact of its own
///
/// A feature key is a **stem**. The other three facts describe the text handed
/// to the stemmer and the shape it is written in; none of them describes the
/// rules it applies.
///
/// This was reachable, and silent.
/// [`Classifier::restore`](crate::Classifier::restore) rebuilds a classifier
/// with [`default_stemmer`](crate::default_stemmer) — English Porter —
/// whatever stemmer trained the model, because a saved model carried no record
/// of which one it was. A French classifier trained through
/// `StemmerOf(PorterStemmerFr)`, saved and restored, therefore keyed its
/// features on French stems and probed them with English ones: `chantait`
/// stems to `chant`, a feature of the trained model, and to `chantait`, which
/// is not one, so the probe read as a token the corpus never contained. Every
/// number in the model stayed valid and the classifier answered confidently.
/// `tests/stemmer_stamp.rs` reproduces exactly that and asserts it is refused.
///
/// The fingerprint is over the stemmer's *behaviour*, not its type name, so it
/// separates a caller's own [`Stemmer`](crate::Stemmer) implementations too.
/// `stamp::tests::every_bridged_stemmer_has_its_own_fingerprint` enumerates all
/// sixteen stemmers `verbora-stemmers` publishes and asserts the sixteen
/// fingerprints are pairwise distinct, so the claim is proved over the shipped
/// set rather than asserted for stemmers in general.
///
/// # What the stamp deliberately does not cover
///
/// The **stop-word list**, which is process-global mutable state in
/// `verbora-stemmers` rather than a fact about the build. `add_document` calls
/// `tokenize_and_stem(text, keep_stops)`, so with `keep_stops` false the
/// feature set depends on what that list held at the moment the call ran —
/// which can differ between two calls in one process, let alone between two
/// processes. A stamp is the wrong instrument for it: it is not a property of
/// the artifact's provenance, and folding it in would make
/// [`ArtifactStamp::for_stemmer`] answer differently at two points in one
/// program's life and refuse a model that same process had just saved.
/// [`stemmer_fingerprint`] therefore probes with `keep_stops = true`, which
/// bypasses the list entirely; a caller who mutates it is responsible for doing
/// so consistently across training and loading.
///
/// # What "refuse" means, and how a user tells the two failures apart
///
/// A corrupt file and an incompatible file need opposite responses — repair or
/// re-fetch the first, retrain the second — so they are different errors.
/// [`StampError`] never reports a parse failure, and the parse failure
/// ([`crate::LoadError::Parse`], [`crate::RestoreError::Parse`]) never reports a
/// version problem.
///
/// Within [`StampError`] the three cases stay distinct for the same reason:
///
/// * [`StampError::Incompatible`] — a well-formed stamp naming a different
///   build. The file is intact and its provenance is known; it just cannot be
///   reused. Both stamps are carried so the message can name them.
/// * [`StampError::Malformed`] — a `_verbora` member that is not a stamp. This
///   is damage, not a version difference, and is the one `StampError` that
///   points at the file rather than at the build.
/// * [`StampError::Missing`] — no `_verbora` member at all.
///
/// # The unstamped model, which is the dangerous one
///
/// A model saved before stamping existed is detectable **only as an absence**.
/// It is a well-formed JSON object with no `_verbora` member, and nothing in it
/// records which Unicode version produced its features — that information was
/// never written down and cannot be recovered from the bytes. It is therefore
/// indistinguishable from a hand-written object, from one produced by a
/// different tool, and from one produced by a hypothetical future build that
/// dropped the stamp.
///
/// Because an unstamped model cannot be validated, it is **refused**, with its
/// own [`StampError::Missing`] variant rather than being folded into
/// `Incompatible`. Refusing is the only safe direction: accepting it would mean
/// guessing a version, and a wrong guess reproduces exactly the silent
/// misprediction the stamp exists to prevent. The distinct variant exists so
/// the message can say "retrain this model" rather than "your file is damaged",
/// which is the response the user actually needs.
///
/// A pre-stamp model cannot accidentally *look* stamped: the top-level members
/// a saved classifier has ever had are the fixed set the reference's
/// `JSON.stringify` emits, and no feature name can reach the top level.
///
/// # Why this duplicates `verbora-tfidf`'s module of the same name
///
/// `verbora-tfidf` carries an equivalent module because it has the same
/// obligation over a different persisted shape. The two are deliberately
/// independent: each crate's [`SCHEMA`] counts *its own* format and pipeline
/// changes, so bumping one does not invalidate the other's artifacts. A shared
/// definition belongs in `verbora-core` once that crate's own migration lands;
/// until then the duplication is the honest form, because a single shared
/// counter would refuse artifacts for changes that never touched them.
///
/// See the [`ArtifactStamp`] for what the three fields cover, what
/// they deliberately do not, and why no one of them alone is sufficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactStamp {
    /// [`SCHEMA`] as of the build that saved the model.
    pub schema: u32,
    /// The Unicode version whose `Word_Break` assignments produced the tokens
    /// the features were stemmed from, as `(major, minor, update)`.
    pub unicode: (u64, u64, u64),
    /// [`lowercase_fingerprint`] as of the build that saved the model.
    ///
    /// `None` for a model whose stamp carries no `lowercase` member, which
    /// every schema-1 model is: the member did not exist when they were saved.
    /// Absence is a version difference rather than damage, so it is represented
    /// here instead of being rejected — [`StampError::Malformed`] stays
    /// reserved for a member that is present and wrong.
    pub lowercase: Option<u64>,
    /// [`stemmer_fingerprint`] of the stemmer that derived the model's feature
    /// keys.
    ///
    /// `None` for an artifact whose keys are not stems — a
    /// [`MaxEntClassifier`](crate::MaxEntClassifier) keys on the context values
    /// it is handed and never stems anything — and for the schema-1 and
    /// schema-2 models that predate the member. As with `lowercase`, absence is
    /// a version difference rather than damage.
    pub stemmer: Option<u64>,
}

impl ArtifactStamp {
    /// The stamp for an artifact whose keys are **not** stems.
    ///
    /// [`MaxEntClassifier`](crate::MaxEntClassifier) is the only such artifact
    /// in this crate: its keys are the context values a caller hands it, so
    /// there is no stemmer to describe and [`Self::stemmer`] is `None`. A
    /// document classifier uses [`Self::for_stemmer`] instead.
    ///
    /// The first call computes [`lowercase_fingerprint`]; every later one in
    /// the same process reads it back from a [`OnceLock`].
    #[must_use]
    pub fn current() -> Self {
        Self {
            schema: SCHEMA,
            unicode: verbora_tokenizers::unicode_version(),
            lowercase: Some(lowercase_fingerprint()),
            stemmer: None,
        }
    }

    /// The stamp for an artifact whose feature keys are `stemmer`'s output.
    ///
    /// This is what a [`Classifier`](crate::Classifier) writes when it saves
    /// and what it demands when it loads, so a model restored under a stemmer
    /// that would key it differently is refused rather than silently
    /// mispredicting.
    #[must_use]
    pub fn for_stemmer(stemmer: &(impl Stemmer + ?Sized)) -> Self {
        Self {
            stemmer: Some(stemmer_fingerprint(stemmer)),
            ..Self::current()
        }
    }

    /// The stamp as the `_verbora` member's value.
    ///
    /// The `lowercase` member is omitted for a stamp that carries no
    /// fingerprint; [`Self::current`]'s never does.
    #[must_use]
    pub fn to_value(self) -> DynValue {
        let (major, minor, update) = self.unicode;
        let mut members = vec![
            ("schema".to_owned(), DynValue::Num(f64::from(self.schema))),
            (
                "unicode".to_owned(),
                DynValue::Str(format!("{major}.{minor}.{update}")),
            ),
        ];
        if let Some(fingerprint) = self.lowercase {
            members.push((
                "lowercase".to_owned(),
                DynValue::Str(format!("{fingerprint:016x}")),
            ));
        }
        if let Some(fingerprint) = self.stemmer {
            members.push((
                "stemmer".to_owned(),
                DynValue::Str(format!("{fingerprint:016x}")),
            ));
        }
        DynValue::Obj(members)
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
            Some(fingerprint) => write!(f, ", lowercase {fingerprint:016x}")?,
            None => f.write_str(", lowercase unrecorded")?,
        }
        match self.stemmer {
            Some(fingerprint) => write!(f, ", stemmer {fingerprint:016x}"),
            None => f.write_str(", no stemmer"),
        }
    }
}

/// Why a saved model was refused on its compatibility stamp.
///
/// This never reports a JSON syntax error: parsing failed or it did not, and
/// [`crate::LoadError`] / [`crate::RestoreError`] keep the two apart so a caller
/// can tell "this file is damaged" from "this file is from another build".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StampError {
    /// The object carries no `_verbora` member.
    ///
    /// Either it predates stamping or it was not written by this library. In
    /// both cases the build behind its features is unrecorded and
    /// unrecoverable, so it cannot be validated and is refused. Retrain from the
    /// source documents.
    Missing,
    /// The `_verbora` member is present but is not a stamp.
    ///
    /// The one variant that indicates a damaged file rather than a version
    /// difference. A stamp with no `lowercase` member is *not* this: that is
    /// what a schema-1 model looks like, and it is reported as
    /// [`Self::Incompatible`] with [`ArtifactStamp::lowercase`] set to `None`.
    Malformed,
    /// The stamp is well formed and names a different build.
    ///
    /// Boxed because the pair is 128 bytes and this variant travels inside
    /// every `Result` the load path returns, where the success case is by far
    /// the common one.
    Incompatible(Box<StampMismatch>),
}

/// The stamp a saved artifact carries and the one this build demands.
///
/// Both are reported because the difference is the diagnosis: a caller who can
/// see that only [`ArtifactStamp::stemmer`] differs knows to reach for
/// [`Classifier::restore_with`](crate::Classifier::restore_with) rather than to
/// retrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StampMismatch {
    /// What the artifact says produced it.
    pub found: ArtifactStamp,
    /// What this build is.
    pub expected: ArtifactStamp,
}

impl std::fmt::Display for StampError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(
                f,
                "saved model carries no `{STAMP_PROPERTY}` compatibility stamp, so the \
                 build its features were derived under is unknown; retrain it \
                 with this version ({})",
                ArtifactStamp::current()
            ),
            Self::Malformed => write!(
                f,
                "saved model has a `{STAMP_PROPERTY}` member that is not a compatibility \
                 stamp (expected {{\"schema\":<integer>,\"unicode\":\"<major>.<minor>.<update>\",\"lowercase\":\"<16 hex digits>\",\"stemmer\":\"<16 hex digits>\"}})"
            ),
            Self::Incompatible(mismatch) => write!(
                f,
                "saved model was written by an incompatible build ({}); this build is \
                 ({}). Its feature keys were derived by a different stemmer, or from text \
                 case-folded or cut into tokens differently, so retrain it from the source \
                 documents rather than loading it",
                mismatch.found, mismatch.expected
            ),
        }
    }
}

impl std::error::Error for StampError {}

/// Reads the stamp out of a parsed model object.
fn read(value: &DynValue) -> Result<ArtifactStamp, StampError> {
    let field = value.get(STAMP_PROPERTY).ok_or(StampError::Missing)?;
    let (Some(DynValue::Num(schema)), Some(DynValue::Str(unicode))) =
        (field.get("schema"), field.get("unicode"))
    else {
        return Err(StampError::Malformed);
    };
    // `JSON.parse` gives every number as an `f64`; only an exact non-negative
    // integer in range is a schema number.
    if !schema.is_finite()
        || schema.fract() != 0.0
        || *schema < 0.0
        || *schema > f64::from(u32::MAX)
    {
        return Err(StampError::Malformed);
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "range and integrality checked immediately above"
    )]
    let schema = *schema as u32;
    // Absent is a schema-1 stamp, which predates the member; present-and-wrong
    // is damage. The two get different answers.
    let lowercase = read_fingerprint(field, "lowercase")?;
    let stemmer = read_fingerprint(field, "stemmer")?;
    Ok(ArtifactStamp {
        schema,
        unicode: parse_unicode(unicode).ok_or(StampError::Malformed)?,
        lowercase,
        stemmer,
    })
}

/// One optional hex-fingerprint member: absent is a version difference,
/// present-and-unparseable is damage.
fn read_fingerprint(field: &DynValue, name: &str) -> Result<Option<u64>, StampError> {
    match field.get(name) {
        None => Ok(None),
        Some(DynValue::Str(s)) => Ok(Some(parse_fingerprint(s).ok_or(StampError::Malformed)?)),
        Some(_) => Err(StampError::Malformed),
    }
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

/// Accepts a parsed model object only if its stamp is this build's.
///
/// # Errors
///
/// [`StampError::Missing`], [`StampError::Malformed`] or
/// [`StampError::Incompatible`] — see each for what a caller should do about
/// it.
pub fn verify_stamp(value: &DynValue) -> Result<(), StampError> {
    verify_stamp_against(value, ArtifactStamp::current())
}

/// Accepts a parsed artifact only if its stamp is `expected`.
///
/// A document classifier passes [`ArtifactStamp::for_stemmer`] for the stemmer
/// it is about to rebuild with, so a model keyed by a different one is refused.
///
/// # Errors
///
/// [`StampError::Missing`], [`StampError::Malformed`] or
/// [`StampError::Incompatible`] — see each for what a caller should do about
/// it.
pub fn verify_stamp_against(value: &DynValue, expected: ArtifactStamp) -> Result<(), StampError> {
    let found = read(value)?;
    if found == expected {
        Ok(())
    } else {
        Err(StampError::Incompatible(Box::new(StampMismatch {
            found,
            expected,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A stamp object, with `lowercase` present exactly when `lowercase` is.
    fn stamped_with(schema: f64, unicode: &str, lowercase: Option<&str>) -> DynValue {
        let mut members = vec![
            ("schema".to_owned(), DynValue::Num(schema)),
            ("unicode".to_owned(), DynValue::Str(unicode.to_owned())),
        ];
        if let Some(lowercase) = lowercase {
            members.push(("lowercase".to_owned(), DynValue::Str(lowercase.to_owned())));
        }
        DynValue::Obj(vec![(STAMP_PROPERTY.to_owned(), DynValue::Obj(members))])
    }

    /// A stamp object carrying this build's own lowercase fingerprint, so the
    /// only thing under test is `schema` and `unicode`.
    fn stamped(schema: f64, unicode: &str) -> DynValue {
        stamped_with(
            schema,
            unicode,
            Some(&format!("{:016x}", lowercase_fingerprint())),
        )
    }

    /// This build's Unicode version, spelled as the JSON member it writes.
    fn current_version() -> String {
        let (major, minor, update) = ArtifactStamp::current().unicode;
        format!("{major}.{minor}.{update}")
    }

    // --- The fingerprint ---------------------------------------------------

    #[test]
    fn the_fingerprint_is_the_documented_walk_of_this_builds_own_mapping() {
        assert_eq!(lowercase_fingerprint(), oracle_fingerprint(real_lowercase));
    }

    /// Every stemmer `verbora-stemmers` publishes has its own fingerprint.
    ///
    /// The claim the stamp rests on is "a model keyed by one stemmer is refused
    /// by another", and this is what proves it over the shipped set: all
    /// fifteen `TokenizeAndStem` implementations `verbora-stemmers` publishes
    /// are enumerated, and the fifteen fingerprints must be pairwise distinct.
    /// Sampling three of them would have passed while any two of the remaining
    /// twelve collided, and a collision is a silently accepted model. The count
    /// is pinned by equality, so a stemmer dropped from the list fails rather
    /// than reporting a clean sweep. (`StemmerJa` is excluded because it does
    /// not implement `TokenizeAndStem` and so cannot key a classifier at all.)
    #[test]
    fn every_bridged_stemmer_has_its_own_fingerprint() {
        use crate::{Stemmer, StemmerOf};

        let stemmers: Vec<(&str, Box<dyn Stemmer>)> = vec![
            (
                "CarryStemmerFr",
                Box::new(StemmerOf(verbora_stemmers::CarryStemmerFr::new())),
            ),
            (
                "LancasterStemmer",
                Box::new(StemmerOf(verbora_stemmers::LancasterStemmer::new())),
            ),
            (
                "PorterStemmer",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmer::new())),
            ),
            (
                "PorterStemmerDe",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerDe::new())),
            ),
            (
                "PorterStemmerEs",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerEs::new())),
            ),
            (
                "PorterStemmerFa",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerFa::new())),
            ),
            (
                "PorterStemmerFr",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerFr::new())),
            ),
            (
                "PorterStemmerIt",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerIt::new())),
            ),
            (
                "PorterStemmerNl",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerNl::new())),
            ),
            (
                "PorterStemmerNo",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerNo::new())),
            ),
            (
                "PorterStemmerPt",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerPt::new())),
            ),
            (
                "PorterStemmerRu",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerRu::new())),
            ),
            (
                "PorterStemmerSv",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerSv::new())),
            ),
            (
                "PorterStemmerUk",
                Box::new(StemmerOf(verbora_stemmers::PorterStemmerUk::new())),
            ),
            (
                "StemmerId",
                Box::new(StemmerOf(verbora_stemmers::StemmerId::new())),
            ),
        ];
        assert_eq!(
            stemmers.len(),
            15,
            "every published stemmer that can key a classifier is enumerated"
        );

        let mut seen: Vec<(&str, u64)> = Vec::with_capacity(stemmers.len());
        for (name, stemmer) in &stemmers {
            let fingerprint = stemmer_fingerprint(&**stemmer);
            if let Some((other, _)) = seen.iter().find(|(_, f)| *f == fingerprint) {
                panic!("{name} and {other} share fingerprint {fingerprint:016x}");
            }
            seen.push((name, fingerprint));
            // …and it is a function of its argument, not of when it was asked.
            assert_eq!(stemmer_fingerprint(&**stemmer), fingerprint, "{name}");
        }
        assert_eq!(seen.len(), 15);

        // The default stemmer is one of them, by behaviour rather than by name.
        assert_eq!(
            stemmer_fingerprint(&*crate::default_stemmer()),
            seen.iter()
                .find(|(name, _)| *name == "PorterStemmer")
                .expect("English Porter is enumerated")
                .1
        );
    }

    /// A stemmer that differs from another on exactly one probe token is still
    /// separated, so the fingerprint is not merely a coarse type tag.
    #[test]
    fn one_changed_token_moves_the_stemmer_fingerprint() {
        use crate::{StemCache, Stemmer};

        struct Shim(bool);
        impl Stemmer for Shim {
            fn tokenize_and_stem(&self, text: &str, keep_stops: bool) -> Vec<String> {
                let mut out = crate::default_stemmer().tokenize_and_stem(text, keep_stops);
                if self.0 && text == STEMMER_PROBES[0] {
                    if let Some(first) = out.first_mut() {
                        first.push('x');
                    }
                }
                out
            }
        }
        let _ = StemCache::new();
        assert_ne!(
            stemmer_fingerprint(&Shim(false)),
            stemmer_fingerprint(&Shim(true))
        );
        assert_eq!(
            stemmer_fingerprint(&Shim(false)),
            stemmer_fingerprint(&*crate::default_stemmer())
        );
    }

    #[test]
    fn the_fingerprint_is_stable_within_a_process() {
        // It is cached in a `OnceLock`, and a stamp that changed between two
        // calls would refuse models this very build saved.
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
        // model trained under one and loaded under the other was accepted.
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
        let differing = CONTEXT_PROBES
            .iter()
            .filter(|probe| {
                let flat: String = probe.chars().flat_map(char::to_lowercase).collect();
                probe.to_lowercase() != flat
            })
            .count();
        assert_eq!(differing, 4);
    }

    #[test]
    fn the_default_stemmer_case_folds_before_it_tokenizes() {
        // Why this crate's stamp needs the fingerprint at all: the fold is in
        // front of the tokenizer, so a changed mapping can move a word boundary
        // and not only a spelling. `İ` (U+0130) lowercases to two scalars —
        // `i` plus U+0307 COMBINING DOT ABOVE — and the token the classifier
        // learns is the folded form, never the raw one.
        let stemmer = crate::default_stemmer();
        assert_eq!(stemmer.tokenize_and_stem("İD", true), vec!["i\u{307}d"]);
        assert_eq!(
            stemmer.tokenize_and_stem("QQQQ", true),
            stemmer.tokenize_and_stem("qqqq", true)
        );
    }

    // --- Reading and refusing ----------------------------------------------

    #[test]
    fn the_current_stamp_round_trips_through_its_own_json() {
        let stamp = ArtifactStamp::current();
        let json = DynValue::Obj(vec![(STAMP_PROPERTY.to_owned(), stamp.to_value())])
            .json_stringify()
            .expect("an object is never undefined");
        assert!(json.contains(r#""lowercase":"#), "{json}");
        let parsed = DynValue::parse(&json).expect("the stamp writer emits valid JSON");
        assert_eq!(verify_stamp(&parsed), Ok(()));
    }

    #[test]
    fn a_stamp_naming_a_foreign_lowercase_mapping_is_refused() {
        // A feature key is a stem of a token cut out of case-folded text, so a
        // stamp naming a different mapping describes different features even
        // when the schema and the tokenizer's Unicode version agree.
        let foreign = format!("{:016x}", lowercase_fingerprint() ^ 1);
        let Err(StampError::Incompatible(mismatch)) = verify_stamp(&stamped_with(
            f64::from(SCHEMA),
            &current_version(),
            Some(&foreign),
        )) else {
            panic!("a foreign lowercase mapping must be refused");
        };
        assert_eq!(mismatch.found.lowercase, Some(lowercase_fingerprint() ^ 1));
        assert_eq!(mismatch.found.schema, mismatch.expected.schema);
        assert_eq!(mismatch.found.unicode, mismatch.expected.unicode);
        assert_eq!(mismatch.expected, ArtifactStamp::current());
    }

    #[test]
    fn a_stamp_with_no_lowercase_member_is_incompatible_not_malformed() {
        // What every schema-1 model looks like. It is intact and its provenance
        // is partly known, so it is a version difference — the response is
        // "retrain it", not "your file is damaged".
        let Err(StampError::Incompatible(mismatch)) =
            verify_stamp(&stamped_with(f64::from(SCHEMA), &current_version(), None))
        else {
            panic!("a stamp with no lowercase member must be Incompatible");
        };
        assert_eq!(mismatch.found.lowercase, None);
        assert_eq!(mismatch.expected, ArtifactStamp::current());
        assert!(
            mismatch.found.to_string().contains("lowercase unrecorded"),
            "{}",
            mismatch.found
        );
    }

    #[test]
    fn an_absent_member_is_missing_not_malformed() {
        assert_eq!(
            verify_stamp(&DynValue::Obj(vec![(
                "features".to_owned(),
                DynValue::Obj(Vec::new())
            )])),
            Err(StampError::Missing)
        );
    }

    #[test]
    fn a_member_that_is_not_a_stamp_is_malformed() {
        let current = ArtifactStamp::current();
        let version = current_version();
        let fingerprint = format!("{:016x}", lowercase_fingerprint());
        for bad in [
            // Not an object at all.
            DynValue::Obj(vec![(
                STAMP_PROPERTY.to_owned(),
                DynValue::Str("17.0.0".to_owned()),
            )]),
            // Unicode version in the wrong shape.
            stamped(f64::from(SCHEMA), "17.0"),
            stamped(f64::from(SCHEMA), "17.0.0.0"),
            stamped(f64::from(SCHEMA), "seventeen"),
            stamped(f64::from(SCHEMA), ""),
            // Schema that is not a whole number in range.
            stamped(1.5, "17.0.0"),
            stamped(-1.0, "17.0.0"),
            stamped(f64::NAN, "17.0.0"),
            // A `lowercase` member that is present and is not sixteen lowercase
            // hex digits. Present-and-wrong is damage; absent is a version
            // difference, and has its own test above.
            stamped_with(f64::from(SCHEMA), &version, Some("")),
            stamped_with(f64::from(SCHEMA), &version, Some("0")),
            stamped_with(f64::from(SCHEMA), &version, Some(&fingerprint[..15])),
            stamped_with(
                f64::from(SCHEMA),
                &version,
                Some(&format!("0{fingerprint}")),
            ),
            stamped_with(f64::from(SCHEMA), &version, Some("0123456789ABCDEF")),
            stamped_with(f64::from(SCHEMA), &version, Some("0x0123456789abcd")),
            stamped_with(f64::from(SCHEMA), &version, Some("0123456789abcdeg")),
            stamped_with(f64::from(SCHEMA), &version, Some(" 123456789abcdef")),
            // …and a `lowercase` member that is not a string at all.
            DynValue::Obj(vec![(
                STAMP_PROPERTY.to_owned(),
                DynValue::Obj(vec![
                    ("schema".to_owned(), DynValue::Num(f64::from(SCHEMA))),
                    ("unicode".to_owned(), DynValue::Str(version.clone())),
                    ("lowercase".to_owned(), DynValue::Num(1.0)),
                ]),
            )]),
        ] {
            assert_eq!(verify_stamp(&bad), Err(StampError::Malformed), "{bad:?}");
        }
        // …and the shape that differs from the above only by being valid.
        assert_eq!(
            verify_stamp(&stamped(f64::from(current.schema), &version)),
            Ok(())
        );
    }

    #[test]
    fn a_foreign_schema_unicode_version_or_mapping_is_incompatible_in_both_directions() {
        let current = ArtifactStamp::current();
        let (major, minor, update) = current.unicode;
        let version = current_version();

        for schema in [current.schema - 1, current.schema + 1] {
            let Err(StampError::Incompatible(mismatch)) =
                verify_stamp(&stamped(f64::from(schema), &version))
            else {
                panic!("schema {schema} must be refused");
            };
            assert_eq!(mismatch.found.schema, schema);
            assert_eq!(mismatch.expected, current);
        }

        for other in [
            format!("{}.{minor}.{update}", major - 1),
            format!("{}.{minor}.{update}", major + 1),
            format!("{major}.{}.{update}", minor + 1),
            format!("{major}.{minor}.{}", update + 1),
        ] {
            assert!(
                matches!(
                    verify_stamp(&stamped(f64::from(current.schema), &other)),
                    Err(StampError::Incompatible(_))
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
                    verify_stamp(&stamped_with(
                        f64::from(current.schema),
                        &version,
                        Some(&format!("{other:016x}"))
                    )),
                    Err(StampError::Incompatible(_))
                ),
                "lowercase {other:016x} must be refused"
            );
        }
    }

    #[test]
    fn the_display_form_names_all_four_facts() {
        let (major, minor, update) = ArtifactStamp::current().unicode;
        assert_eq!(
            ArtifactStamp::current().to_string(),
            format!(
                "schema {SCHEMA}, Unicode {major}.{minor}.{update}, lowercase {:016x}, no stemmer",
                lowercase_fingerprint()
            )
        );
        let stemmer = crate::default_stemmer();
        assert_eq!(
            ArtifactStamp::for_stemmer(&*stemmer).to_string(),
            format!(
                "schema {SCHEMA}, Unicode {major}.{minor}.{update}, lowercase {:016x}, stemmer {:016x}",
                lowercase_fingerprint(),
                stemmer_fingerprint(&*stemmer),
            )
        );
    }
}
