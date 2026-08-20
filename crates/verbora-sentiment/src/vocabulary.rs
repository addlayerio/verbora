//! The word → polarity tables, and how they are loaded, indexed and stemmed.
//!
//! # The lookup form
//!
//! A lexicon key is a piece of text, not a token. The shipped tables spell one
//! entry `cover-up`, another `bad luck`, another `Abfall`, and a token stream
//! contains none of those: [`WordTokenizer`] cuts at UAX #29 word boundaries,
//! so it yields `cover`, `up`, `bad`, `luck`, `Abfall`. Looking a key up by the
//! text it was written with therefore finds almost none of the interesting
//! ones. Verbora indexes by a form both sides can reach instead:
//!
//! > The **pieces** of a string are its [`WordTokenizer`] word segments, each
//! > lowercased. Its **lookup form** is those pieces joined by one U+0020 —
//! > or, for a string with no word segment at all, the string lowercased.
//!
//! ```text
//! "cover-up"       pieces ["cover", "up"]            form "cover up"
//! "bad luck"       pieces ["bad", "luck"]            form "bad luck"
//! "Abfall"         pieces ["abfall"]                 form "abfall"
//! "son-of-a-bitch" pieces ["son","of","a","bitch"]   form "son of a bitch"
//! "😂"             pieces []                         form "😂"
//! ```
//!
//! Every entry is indexed under its lookup form. `cover-up` and `cover up` are
//! therefore the same key, and 14,273 of the 75,803 shipped entries — every
//! entry the tokenizer cuts into two or more pieces — become reachable from a
//! token stream instead of being dead weight in the binary. So does every
//! capitalised entry, which matters most for `pattern`/German, where 1,234 of
//! 3,465 keys ship capitalised.
//!
//! That 14,273 is counted by walking [`WordTokenizer`] over every key, which is
//! the only method that answers the question asked. A `key.contains('-') ||
//! key.contains(' ')` filter — the obvious shortcut, and the one this number
//! used to be derived with — gets 14,268: it misses eight keys the tokenizer
//! splits at something else (`s/n`, `señal/ruido`, `signal/noise`,
//! `encender(se)`, `rompre´s`, `cal”ligrafia`, `passarel”la`, `f*cking`) and
//! claims three it does not split at all (`ultratge `, `herri-`,
//! `azkartasun `, whose hyphen or space is at an edge). Two errors of opposite
//! sign in one filter is why the count is enumerated by
//! `tests/key_derivation.rs` rather than restated here.
//!
//! Two keys can share a lookup form: English senticon ships both `pitch-black`
//! and `pitch black`, and `pattern`/German ships both `Stolz` and `stolz` with
//! *different* polarities. **The later entry in file order wins**, and the
//! earlier one is shadowed — the same rule stem collisions have always
//! followed, for the same reason. Across all fourteen tables 102 entries are
//! shadowed this way; [`Vocabulary::len`] still counts them, because the table
//! really does contain them.
//!
//! # Insertion order is load-bearing
//!
//! Both collision rules above resolve *last-wins in file order*, so the order
//! entries are loaded in is part of the answer. When a stemmer is supplied the
//! whole table is rebuilt piece by piece:
//!
//! ```text
//! for entry in table:  rebuilt[pieces(entry.key).map(stem).join(" ")] = entry.value
//! ```
//!
//! For English AFINN with the Porter stemmer that is 3,382 keys collapsing to
//! 1,967, with collisions that change the stored polarity — `affect`(-1) loses
//! to `affection`(3), `arrested`(-3) loses to `arrests`(-2). A `HashMap`
//! iteration order picks a different winner for each of those and a `BTreeMap`
//! picks a third set, so [`Vocabulary`] keeps its entries in a `Vec` in
//! insertion order and the hash map only maps a lookup form to a slot.
//!
//! # One derivation, two callers
//!
//! The line above is the whole rule, and the thing to notice about it is that
//! the scoring loop must compute *the same string* from a run of tokens. Two
//! spellings of one rule is how they stop agreeing: this table was indexed by
//! re-segmenting the stemmer's output and probed with the stemmer's output
//! verbatim, so `ofendre's` stemmed to `ofendre'`, was filed under `ofendre`,
//! and answered for the unrelated key `ofendre`.
//!
//! There is therefore exactly one definition of each half, and both sides call
//! it: [`lowercase`] and [`stem_piece`] derive a piece, [`write_form`] joins
//! pieces into a form, and nothing derives a form from a form. That last clause
//! is not decorative — [`Vocabulary::get`] broke it by reducing its argument
//! before probing, so on a rebuilt table `get("ne'")` answered the unrelated
//! key `ne`, whose polarity has the opposite sign. It now offers the argument
//! to the index verbatim first and only reduces a string the table does not
//! already hold.
//! `tests/key_derivation.rs` walks every key of every table through both
//! callers, for every stemmer that can be installed, and asserts they agree.
//!
//! # What a value keeps
//!
//! A [`Polarity`] carries both the source file's own text and its parsed
//! `f64`. Senticon and pattern write theirs as decimal strings (`"0.813"`,
//! `"-0.30"`) and AFINN as integers, so [`Polarity::as_written`] reports how a
//! value was published and [`Polarity::value`] reports the number scoring uses.
//! The parse happens once, when the table is built, so `value` is a field read
//! and can never answer `NaN`.
//!
//! [`WordTokenizer`]: verbora_tokenizers::WordTokenizer

use std::borrow::Cow;
use std::sync::OnceLock;

use rustc_hash::FxHashMap;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

use crate::analyzer::lowercase;
use crate::data;
use crate::stemmer::Stemmer;
use crate::{Language, VocabularyKind};

/// The stem of an **already lowercased** piece, or the piece itself when the
/// stemmer returns nothing at all.
///
/// This is the only place a stem becomes a piece. [`Forms::stemmed`] derives
/// the pieces of every key with it when a table is rebuilt, and
/// [`Contributions`] derives the pieces of every token with it when text is
/// scored, so the two cannot drift apart.
///
/// The empty-stem fallback lives here rather than at either call site for the
/// same reason: a piece that vanished would change a key's piece count, and
/// applying that rule on the table side only is what left the rebuild storing
/// a piece the scoring loop would never produce.
///
/// [`Contributions`]: crate::Contributions
pub(crate) fn stem_piece<'a, S: Stemmer + ?Sized>(lower: &'a str, stemmer: &S) -> Cow<'a, str> {
    let stem = stemmer.stem(lower);
    if stem.is_empty() {
        Cow::Borrowed(lower)
    } else {
        stem
    }
}

/// Writes the lookup form of `pieces` — the pieces joined by one U+0020 — into
/// `out`, replacing whatever it held, and returns how many pieces it wrote.
///
/// **This is the only place a lookup form is spelled.** [`Forms`] builds a
/// table's keys with it and [`Contributions`] builds the scoring loop's probes
/// with it, over pieces derived on both sides by [`lowercase`] and
/// [`stem_piece`]. An index built here can therefore only be probed with a
/// string built here — which is the property that was missing when a stemmed
/// table was indexed under one spelling and consulted with another.
///
/// A one-piece form is that piece verbatim, because a single element is
/// written with no separator before it and none after. Both callers rely on
/// that to borrow the piece instead of copying it, and
/// `tests/key_derivation.rs` enumerates the identity rather than assuming it.
///
/// [`Contributions`]: crate::Contributions
pub(crate) fn write_form<I>(out: &mut String, pieces: I) -> usize
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    out.clear();
    let mut count = 0usize;
    for piece in pieces {
        if count > 0 {
            out.push(' ');
        }
        out.push_str(piece.as_ref());
        count += 1;
    }
    count
}

/// How one word segment becomes one piece of a lookup form.
///
/// Two implementations, one per table shape: a shipped table indexes its keys
/// [`AsWritten`], a stemmed rebuild indexes them [`Stemming`]. Both are
/// compositions of [`lowercase`] and [`stem_piece`] and nothing else, which is
/// the point — [`Forms`] is written once against this trait instead of once
/// per shape, so the two shapes cannot acquire different segmentation,
/// different casing or a different empty-piece rule.
trait Piece {
    /// The piece one word segment contributes to a lookup form.
    fn of<'a>(&self, segment: &'a str) -> Cow<'a, str>;

    /// The extra spelling `text` should also answer under, if it has one.
    ///
    /// Only the as-written index does. A source file gives a key exactly two
    /// spellings — the pieces the tokenizer cuts it into, and the key's own
    /// untokenized text — and the alias is the second of those, so a caller
    /// who hands `Cover-Up` over whole still finds `cover up`.
    ///
    /// A rebuilt table's keys are not source text: they are lookup forms this
    /// same trait produced, and the scoring loop reaches them by producing the
    /// identical string. A third spelling derived by stemming the *whole* key
    /// would be one no caller computes, would hand a word-level algorithm a
    /// phrase (see [`Vocabulary::stemmed`]), and — measured over the shipped
    /// tables — collides with three genuine keys and displaces them.
    fn alias<'a>(&self, text: &'a str) -> Option<Cow<'a, str>>;
}

/// The piece of a key indexed as the source file spells it.
struct AsWritten;

impl Piece for AsWritten {
    fn of<'a>(&self, segment: &'a str) -> Cow<'a, str> {
        lowercase(segment)
    }

    fn alias<'a>(&self, text: &'a str) -> Option<Cow<'a, str>> {
        Some(lowercase(text))
    }
}

/// The piece of a key indexed through a stemmer.
struct Stemming<'s, S: ?Sized>(&'s S);

impl<S: Stemmer + ?Sized> Piece for Stemming<'_, S> {
    fn of<'a>(&self, segment: &'a str) -> Cow<'a, str> {
        match lowercase(segment) {
            Cow::Borrowed(lower) => stem_piece(lower, self.0),
            // The stem may borrow the lowercased copy, which dies with this
            // frame, so it has to be taken by value.
            Cow::Owned(lower) => Cow::Owned(stem_piece(&lower, self.0).into_owned()),
        }
    }

    fn alias<'a>(&self, _text: &'a str) -> Option<Cow<'a, str>> {
        None
    }
}

/// A polarity: the number a lexicon assigns a key, plus the text the lexicon
/// published it as.
///
/// The two are kept together because the three families really do disagree
/// about how a value is written — AFINN writes integers, ML-SentiCon's `pol`
/// and Pattern's `polarity` are decimal strings — and collapsing everything to
/// `f64` would throw away the only record of how a value was published.
///
/// The fields are private and there is no public constructor, which is what
/// makes [`Self::value`] total: every `Polarity` in existence came from a
/// shipped table whose text was parsed to a finite `f64` when the table was
/// built, so no caller can produce one that would answer `NaN`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Polarity {
    value: f64,
    as_written: Option<&'static str>,
}

impl Polarity {
    /// The value as an `f64`. Always finite.
    ///
    /// This is what scoring uses. The decimal text was parsed once when the
    /// table was built, so reading it costs nothing.
    #[must_use]
    pub fn value(self) -> f64 {
        self.value
    }

    /// The source file's own text, for the families that publish decimal
    /// strings — ML-SentiCon and Pattern. `None` for AFINN, which publishes
    /// integers.
    #[must_use]
    pub fn as_written(self) -> Option<&'static str> {
        self.as_written
    }
}

impl std::fmt::Display for Polarity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.as_written {
            Some(text) => f.write_str(text),
            None => write!(f, "{}", self.value),
        }
    }
}

/// One `key -> polarity` pair.
///
/// `raw` always points into the embedded blob — rebuilding a table rewrites
/// keys, never values — so a stemmed table still reports the source file's own
/// text for a polarity.
#[derive(Debug, Clone)]
struct Entry {
    key: Cow<'static, str>,
    raw: &'static str,
    value: f64,
}

/// What one lookup form resolves to.
///
/// A form can be both — `good` is a key of English AFINN *and* the first piece
/// of `good luck` — so the two answers share a slot and one hash probe serves
/// both. That is the whole reason span matching costs the scoring loop a
/// branch rather than a second lookup per token.
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    /// The entry this form is the lookup form of, if it is one.
    entry: Option<u32>,
    /// The piece count of the longest key whose first piece is this form, or
    /// `0` when no multi-piece key starts here.
    span: u8,
}

/// A word → polarity table: one of the fourteen shipped vocabularies, or the
/// stemmed rebuild of one.
///
/// # Memory
///
/// Keys of a shipped table are `&'static str` slices of the embedded blob, so
/// the table itself is a `Vec` of fat pointers plus a hash map of the same. A
/// lookup form that is byte-identical to its key — which is every ordinary
/// lowercase single-word entry, the overwhelming majority — is stored as that
/// same borrow, so only the keys whose spelling differs from their lookup form
/// (hyphenated, capitalised, oddly spaced) and the first pieces of multi-piece
/// keys own any string data. For the largest vocabulary, English senticon,
/// that is a few thousand short strings, and it is built at most once per
/// process.
#[derive(Debug, Clone)]
pub struct Vocabulary {
    kind: VocabularyKind,
    entries: Vec<Entry>,
    index: FxHashMap<Cow<'static, str>, Slot>,
}

impl Vocabulary {
    /// Which family this table belongs to.
    ///
    /// A rebuild through [`Self::stemmed`] keeps its base table's family, so
    /// this answers the same either way.
    #[must_use]
    pub fn kind(&self) -> VocabularyKind {
        self.kind
    }

    /// Number of entries, including any shadowed by a later entry with the
    /// same lookup form.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The polarity of `word`, or `None` when the table has no such entry.
    ///
    /// `word` is matched by its **lookup form** (see the module
    /// documentation), so every spelling that reduces to the same pieces finds
    /// the same entry: `get("Cover-Up")`, `get("cover-up")` and
    /// `get("cover up")` are one lookup. When two entries share a lookup form
    /// the later one in file order answers.
    ///
    /// This is the whole lookup surface. A separate "just the number" entry
    /// point used to exist beside it; [`Polarity`] carries its `f64` in a
    /// field rather than parsing on demand, so the two would have been the
    /// same work under two names.
    ///
    /// On a table rebuilt by [`Self::stemmed`] the keys are lookup forms rather
    /// than source text, and `word` is matched against them **verbatim first**:
    /// deriving a lookup form from something that already is one can spell a
    /// *different* key, which is the one thing the module documentation says
    /// nothing here may do. With `PorterStemmerFr` installed, English senticon
    /// holds both `ne'` and `ne` with opposite signs, and reducing `ne'` gives
    /// `ne`. Only a string this table does not already hold is treated as text
    /// and reduced, so the second step can never steal an answer from the
    /// first.
    ///
    /// ```
    /// use verbora_sentiment::{Language, Vocabulary, VocabularyKind};
    ///
    /// let afinn = Vocabulary::shared(VocabularyKind::Afinn, Language::English).unwrap();
    /// assert_eq!(afinn.get("cover-up").map(|p| p.value()), Some(-3.0));
    /// assert_eq!(afinn.get("Cover Up").map(|p| p.value()), Some(-3.0));
    /// assert_eq!(afinn.get("no-such-word"), None);
    /// ```
    #[must_use]
    pub fn get(&self, word: &str) -> Option<Polarity> {
        Some(self.polarity_of(self.entry(word)?))
    }

    /// The [`Polarity`] an entry publishes, in the shape its family uses.
    fn polarity_of(&self, entry: &Entry) -> Polarity {
        Polarity {
            value: entry.value,
            as_written: self.values_are_text().then_some(entry.raw),
        }
    }

    /// The entry `word` names, as a lookup form first and as text second.
    ///
    /// **A form is never derived from a form.** A rebuilt table's keys are
    /// lookup forms this crate produced, not source text, and re-segmenting one
    /// can spell a *different* key: with `PorterStemmerFr` installed, English
    /// senticon holds both `ne'` and `ne` with opposite signs, and deriving a
    /// form from `ne'` gives `ne`. So the argument is offered to the index
    /// verbatim first, and only a string that is not already a key of this
    /// table is treated as text and reduced.
    ///
    /// The second step cannot steal an answer from the first, because the first
    /// wins; and on a shipped table it changes nothing at all, since every key
    /// there is `Forms::of` of something and that function is idempotent on its
    /// own output. `tests/stemmed_lookup.rs` enumerates every stored form of
    /// every table under every installable stemmer — 1,151,809 of them — and
    /// asserts each finds its own entry.
    fn entry(&self, word: &str) -> Option<&Entry> {
        if let Some(entry) = self.by_form(word) {
            return Some(entry);
        }
        let forms = Forms::of(word);
        self.by_form(forms.primary.as_ref())
    }

    /// The entry an already-computed lookup form resolves to.
    fn by_form(&self, form: &str) -> Option<&Entry> {
        let slot = self.index.get(form)?;
        self.entries.get(slot.entry? as usize)
    }

    /// The polarity of an already-computed lookup form. The scoring hot path:
    /// one hash probe, no segmentation and no allocation.
    pub(crate) fn form_polarity(&self, form: &str) -> Option<f64> {
        Some(self.by_form(form)?.value)
    }

    /// The piece count of the longest key whose first piece is `form`, or `0`
    /// when no multi-piece key starts there.
    ///
    /// The scoring loop reads this from the same slot as the single-token
    /// polarity, so a token that begins no phrase costs nothing extra.
    pub(crate) fn span_from(&self, form: &str) -> u8 {
        self.index.get(form).map_or(0, |slot| slot.span)
    }

    /// Whether this family publishes its polarities as decimal strings.
    fn values_are_text(&self) -> bool {
        matches!(
            self.kind,
            VocabularyKind::Senticon | VocabularyKind::Pattern
        )
    }

    /// The keys in file order, spelled as the source lexicon spells them.
    ///
    /// This is the source text, not the lookup form: `pattern`/German yields
    /// `Abfall`, and English AFINN yields `cover-up`.
    ///
    /// A table rebuilt by [`Self::stemmed`] has no source text left, so its
    /// keys are its lookup forms — the strings the scoring loop probes with,
    /// one per surviving stem collision rather than one per source entry.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> {
        self.entries.iter().map(|e| e.key.as_ref())
    }

    /// The entries in file order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, Polarity)> {
        let text = self.values_are_text();
        self.entries.iter().map(move |e| {
            (
                e.key.as_ref(),
                Polarity {
                    value: e.value,
                    as_written: text.then_some(e.raw),
                },
            )
        })
    }

    /// The shipped table for `(kind, language)`, or `None` if no such pair
    /// exists.
    ///
    /// Each table is decoded and indexed at most once per process and shared
    /// thereafter.
    #[must_use]
    pub fn shared(kind: VocabularyKind, language: Language) -> Option<&'static Self> {
        Some(load(source_index(kind, language)?))
    }

    /// This table rebuilt through `stemmer`.
    ///
    /// **The stemmer is applied to each piece of a key, not to the key's whole
    /// text.** A stemmer is a word-level algorithm: handing it `well behaved`
    /// makes it strip the `-ed` off a string it has no reason to believe is one
    /// word, and handing it `Abfall` makes it stem a capitalised form no
    /// lowercased token can ever match. Stemming `["well", "behaved"]` and
    /// rejoining is the operation that means anything, and it is what keeps a
    /// phrase key reachable after the rebuild.
    ///
    /// A piece whose stem is empty keeps its own spelling. That rule lives in
    /// one place — the crate-internal `stem_piece` — so that the scoring loop
    /// obeys it too.
    ///
    /// Colliding stems resolve last-wins in this table's order; see the module
    /// documentation for why that is not negotiable.
    ///
    /// # A rebuilt key *is* its lookup form
    ///
    /// The entry a rebuild stores is keyed by the lookup form of its stemmed
    /// pieces — the pieces joined by one U+0020 — and that string is indexed
    /// verbatim rather than being segmented a second time. It has to be: the scoring loop probes
    /// with the same function over the same pieces, so any second pass over the
    /// result would be a spelling only one of the two sides knows about. That
    /// second pass is what filed `ofendre's` — Porter-stemmed to `ofendre'` —
    /// under `ofendre`, where it answered for the unrelated key of that name.
    ///
    /// # The rebuild replaces keys, and that has a cost
    ///
    /// A stemmed table holds stems and nothing else, so the scoring loop's
    /// *unstemmed* first lookup can land on some other word's stem. In
    /// `pattern`/German, `habgierig` stems to `habgier`, and the token
    /// `Habgier` lowercases to exactly that — so with `PorterStemmerDe`
    /// installed it scores `habgierig`'s -0.3059 rather than its own -0.0532.
    /// This is inherent to rebuilding rather than augmenting the table, and it
    /// is why a stemmer is a deliberate choice, not a free improvement: an
    /// analyzer without one has no such crosstalk. It costs 121 of English
    /// AFINN's 3,382 entries their own polarity, a count
    /// `tests/key_derivation.rs` re-derives rather than quotes.
    ///
    /// It is also the expensive part of construction: exactly one
    /// [`Stemmer::stem`] call per piece of every key and not one more — 33,874
    /// of them for English senticon's 24,839 entries — so build one analyzer
    /// and reuse it.
    #[must_use]
    pub fn stemmed<S: Stemmer + ?Sized>(&'static self, stemmer: &S) -> Self {
        let mut build = Build::new(self.kind, self.entries.len());
        for entry in &self.entries {
            // A key the stemmer left alone keeps its `&'static str` borrow into
            // the embedded blob, so the vast majority of the 75,803 shipped
            // keys cost nothing to carry through the rebuild.
            let forms = Forms::stemmed(entry.key.as_ref(), stemmer);
            let key = forms.primary.clone();
            build.insert(key, forms, entry.raw, entry.value);
        }
        build.finish()
    }
}

/// The lookup forms of one string, under one [`Piece`] derivation.
///
/// `primary` is the lookup form proper: [`write_form`] over the string's
/// pieces. `alias` is [`Piece::alias`] — the key's own untokenized text, for
/// the as-written index only — so a caller who hands `Cover-Up` over whole
/// still finds `cover up`; aliases are resolved *through* the primary form
/// when the table is finished, so the two can never disagree about which entry
/// wins a collision. `starter` is the first piece of a multi-piece key, which
/// is where the span scan begins.
///
/// Every field comes from the same `Piece`, so a table and its scoring loop
/// cannot end up with two spellings of one key.
struct Forms<'a> {
    primary: Cow<'a, str>,
    alias: Option<Cow<'a, str>>,
    starter: Option<Cow<'a, str>>,
    pieces: u8,
}

impl<'a> Forms<'a> {
    /// The forms of a key as the source lexicon spells it.
    fn of(text: &'a str) -> Self {
        Self::build(text, &AsWritten)
    }

    /// The forms of a key with every piece stemmed.
    fn stemmed<S: Stemmer + ?Sized>(text: &'a str, stemmer: &S) -> Self {
        Self::build(text, &Stemming(stemmer))
    }

    fn build<P: Piece>(text: &'a str, piece: &P) -> Self {
        let mut segments = WordTokenizer.tokens(text);
        let Some(first) = segments.next() else {
            // No word segment: an emoji, a circled letter, a bare symbol. The
            // whole string is its own single piece, and no tokenizer that
            // filters to words will ever produce it.
            return Self {
                primary: piece.of(text),
                alias: None,
                starter: None,
                pieces: 1,
            };
        };
        let first_piece = piece.of(first);
        let Some(second) = segments.next() else {
            // One segment, so the form is that one piece — `write_form` over a
            // one-element sequence writes exactly its element. When the segment
            // covers the whole string the alias would be that same piece by
            // definition, so it is not derived at all.
            let alias = (first.len() != text.len())
                .then(|| piece.alias(text))
                .flatten()
                .filter(|whole| *whole != first_piece);
            return Self {
                primary: first_piece,
                alias,
                starter: None,
                pieces: 1,
            };
        };
        let mut joined = String::with_capacity(text.len());
        let rest = std::iter::once(second)
            .chain(segments)
            .map(|segment| piece.of(segment));
        let pieces = write_form(
            &mut joined,
            std::iter::once(first_piece.clone()).chain(rest),
        );
        let alias = piece.alias(text).filter(|whole| whole.as_ref() != joined);
        Self {
            primary: Cow::Owned(joined),
            alias,
            starter: Some(first_piece),
            pieces: u8::try_from(pieces).unwrap_or(u8::MAX),
        }
    }
}

/// Accumulates a table, then resolves aliases once every entry is known.
struct Build {
    voca: Vocabulary,
    /// `(alias, primary)` pairs, resolved in [`Build::finish`] so that an
    /// alias always answers with whichever entry won its primary form.
    aliases: Vec<(Cow<'static, str>, Cow<'static, str>)>,
}

impl Build {
    fn new(kind: VocabularyKind, capacity: usize) -> Self {
        Self {
            voca: Vocabulary {
                kind,
                entries: Vec::with_capacity(capacity),
                index: FxHashMap::with_capacity_and_hasher(capacity, rustc_hash::FxBuildHasher),
            },
            aliases: Vec::new(),
        }
    }

    /// Adds one entry under `forms`, the lookup forms its caller derived.
    ///
    /// Re-adding a key that is already present keeps its position and replaces
    /// its value; a *different* key with the same lookup form is appended and
    /// shadows the earlier one.
    ///
    /// **The forms are an argument, not a derivation.** Re-deriving them from
    /// `key` here would mean a stemmed rebuild — whose key is already the
    /// output of one derivation — got a second pass no caller of the scoring
    /// loop applies, which is exactly how the table came to be indexed under
    /// spellings it was never probed with.
    fn insert(
        &mut self,
        key: Cow<'static, str>,
        forms: Forms<'static>,
        raw: &'static str,
        value: f64,
    ) {
        let existing = self
            .voca
            .index
            .get(forms.primary.as_ref())
            .and_then(|slot| slot.entry)
            .filter(|&slot| {
                self.voca
                    .entries
                    .get(slot as usize)
                    .is_some_and(|entry| entry.key == key)
            });
        let slot = match existing {
            Some(slot) => {
                if let Some(entry) = self.voca.entries.get_mut(slot as usize) {
                    entry.raw = raw;
                    entry.value = value;
                }
                slot
            }
            None => {
                let slot = u32::try_from(self.voca.entries.len()).expect("vocabulary fits in u32");
                self.voca.entries.push(Entry { key, raw, value });
                slot
            }
        };
        self.voca
            .index
            .entry(forms.primary.clone())
            .or_default()
            .entry = Some(slot);
        if let Some(alias) = forms.alias {
            self.aliases.push((alias, forms.primary));
        }
        if let Some(starter) = forms.starter {
            let slot = self.voca.index.entry(starter).or_default();
            slot.span = slot.span.max(forms.pieces);
        }
    }

    fn finish(mut self) -> Vocabulary {
        for (alias, primary) in self.aliases {
            let winner = self
                .voca
                .index
                .get(primary.as_ref())
                .and_then(|slot| slot.entry);
            if winner.is_some() {
                self.voca.index.entry(alias).or_default().entry = winner;
            }
        }
        self.voca
    }
}

/// The row of the shipped table for `(kind, language)`.
pub(crate) fn source_index(kind: VocabularyKind, language: Language) -> Option<usize> {
    data::SOURCES
        .iter()
        .position(|s| s.kind == kind && s.language == language)
}

/// Every table is decoded lazily and kept for the process: 1.2 MB of packed
/// pairs, decoded and indexed on first use of that one language.
static CACHE: [OnceLock<Vocabulary>; data::SOURCE_COUNT] =
    [const { OnceLock::new() }; data::SOURCE_COUNT];

/// Decodes one blob: `key \0 polarity \0`, entries in source order.
pub(crate) fn load(index: usize) -> &'static Vocabulary {
    let slot = CACHE.get(index).expect("source index is in range");
    slot.get_or_init(|| {
        let source = data::SOURCES.get(index).expect("source index is in range");
        let mut build = Build::new(source.kind, source.entries);
        let Some(blob) = source.blob else {
            return build.finish();
        };
        // The generator proves the blob is UTF-8, NUL-terminated and has an even
        // number of fields on every run, so a malformed one is a build bug.
        let text = std::str::from_utf8(blob).expect("generated blob is valid UTF-8");
        let mut fields = text.split_terminator('\0');
        while let Some(key) = fields.next() {
            let raw = fields.next().expect("generated blob has paired fields");
            let value = raw.parse().expect("generated polarity is a plain decimal");
            build.insert(Cow::Borrowed(key), Forms::of(key), raw, value);
        }
        let voca = build.finish();
        debug_assert_eq!(voca.len(), source.entries);
        voca
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn afinn_values_are_numbers_and_senticon_values_are_text() {
        let afinn = Vocabulary::shared(VocabularyKind::Afinn, Language::English).unwrap();
        assert_eq!(afinn.get("good").map(Polarity::value), Some(3.0));
        assert_eq!(afinn.get("good").unwrap().as_written(), None);

        let senticon = Vocabulary::shared(VocabularyKind::Senticon, Language::English).unwrap();
        assert_eq!(senticon.get("good").unwrap().as_written(), Some("0.813"));
        assert_eq!(senticon.get("good").unwrap().value(), 0.813);
    }

    #[test]
    fn tables_are_shared_not_rebuilt() {
        let a = Vocabulary::shared(VocabularyKind::Pattern, Language::Dutch).unwrap();
        let b = Vocabulary::shared(VocabularyKind::Pattern, Language::Dutch).unwrap();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn every_shipped_table_decodes_to_its_recorded_size() {
        for (i, source) in data::SOURCES.iter().enumerate() {
            let voca = load(i);
            assert_eq!(
                voca.len(),
                source.entries,
                "{:?}/{}",
                source.kind,
                source.language
            );
            assert_eq!(voca.keys().len(), source.entries);
        }
    }

    #[test]
    fn keys_are_in_source_file_order() {
        let afinn = Vocabulary::shared(VocabularyKind::Afinn, Language::English).unwrap();
        let first: Vec<&str> = afinn.keys().take(4).collect();
        assert_eq!(first, ["abandon", "abandoned", "abandons", "abducted"]);
    }

    #[test]
    fn unknown_pairs_have_no_table() {
        assert!(Vocabulary::shared(VocabularyKind::Afinn, Language::Dutch).is_none());
        assert!(Vocabulary::shared(VocabularyKind::Pattern, Language::Basque).is_none());
        assert!(
            Vocabulary::shared(VocabularyKind::AfinnFinancialMarketNews, Language::English)
                .is_some()
        );
    }

    #[test]
    fn financial_market_news_is_empty_by_construction() {
        let voca = Vocabulary::shared(VocabularyKind::AfinnFinancialMarketNews, Language::English)
            .unwrap();
        assert!(voca.is_empty());
        assert_eq!(voca.get("bankruptcy"), None);
    }

    /// The lookup form, spelled out on the four shapes the tables contain.
    #[test]
    fn a_key_is_found_by_every_spelling_of_its_lookup_form() {
        let afinn = Vocabulary::shared(VocabularyKind::Afinn, Language::English).unwrap();
        for spelling in ["cover-up", "Cover-Up", "cover up", "COVER UP"] {
            assert_eq!(
                afinn.get(spelling).map(Polarity::value),
                Some(-3.0),
                "{spelling}"
            );
        }
        // A capitalised key is reachable from its lowercase, which is what the
        // scoring loop looks a token up by.
        let de = Vocabulary::shared(VocabularyKind::Pattern, Language::German).unwrap();
        assert_eq!(
            de.get("abfall").map(Polarity::value),
            de.get("Abfall").map(Polarity::value)
        );
        assert!(de.get("abfall").map(Polarity::value).is_some());
        // A key with no word segment at all is matched literally.
        let es = Vocabulary::shared(VocabularyKind::Afinn, Language::Spanish).unwrap();
        assert_eq!(es.get("😂").map(Polarity::value), Some(1.0));
    }

    /// Two entries sharing a lookup form: the later one answers, both are
    /// counted, and the shadowed one is still listed by `keys()`.
    #[test]
    fn colliding_lookup_forms_resolve_last_wins_in_file_order() {
        let de = Vocabulary::shared(VocabularyKind::Pattern, Language::German).unwrap();
        let mut order: HashMap<&str, usize> = HashMap::new();
        for (i, key) in de.keys().enumerate() {
            order.insert(key, i);
        }
        // `Stolz` and `stolz` ship with different polarities.
        let (upper, lower) = (order["Stolz"], order["stolz"]);
        let winner = if upper > lower { "Stolz" } else { "stolz" };
        let expected = de
            .iter()
            .find(|(k, _)| *k == winner)
            .expect("both keys ship")
            .1;
        assert_eq!(de.get("stolz").map(Polarity::value), Some(expected.value()));
        assert_eq!(de.get("Stolz").map(Polarity::value), Some(expected.value()));
        assert_ne!(order["Stolz"], order["stolz"]);
    }

    /// The collapse a stemmer causes, with the expected count computed here
    /// from the documented rule rather than copied out of a previous run.
    #[test]
    fn stem_collisions_resolve_last_wins_in_file_order() {
        let base = Vocabulary::shared(VocabularyKind::Afinn, Language::English).unwrap();
        let stemmer = verbora_stemmers::PorterStemmer::new();
        let stemmed = base.stemmed(&stemmer);
        assert_eq!(base.len(), 3382);

        // The rule: every piece of a key is stemmed and the pieces rejoined,
        // so keys with equal stem forms collapse into one entry.
        let mut distinct: HashMap<String, f64> = HashMap::new();
        for (key, polarity) in base.iter() {
            let form = WordTokenizer
                .tokens(key)
                .map(|piece| stemmer.stem(&piece.to_lowercase()).into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            distinct.insert(form, polarity.value());
        }
        assert_eq!(stemmed.len(), distinct.len());
        assert!(stemmed.len() < base.len());
        for (form, expected) in &distinct {
            assert_eq!(
                stemmed.get(form).map(Polarity::value),
                Some(*expected),
                "{form}"
            );
        }

        // `affection`(3) and `arrested`(-3) both collide, and both are followed
        // in file order by another key with the same stem. Last one wins.
        assert_eq!(base.get("affection").map(Polarity::value), Some(3.0));
        assert_eq!(stemmed.get("affect").map(Polarity::value), Some(3.0));
        assert_eq!(base.get("arrested").map(Polarity::value), Some(-3.0));
        assert_eq!(base.get("arrests").map(Polarity::value), Some(-2.0));
        assert_eq!(stemmed.get("arrest").map(Polarity::value), Some(-2.0));

        // Values still point at the source file's own text, so a stemmed
        // senticon table still answers with a decimal string — of whichever
        // entry won the collision, which need not be `good` itself.
        let senticon = Vocabulary::shared(VocabularyKind::Senticon, Language::English)
            .unwrap()
            .stemmed(&verbora_stemmers::PorterStemmer::new());
        let good = senticon.get("good").expect("`good` stems to itself");
        let written = good.as_written().expect("senticon publishes decimals");
        assert_eq!(written.parse::<f64>(), Ok(good.value()));
    }

    /// A phrase key survives the rebuild, because the stemmer sees its pieces.
    #[test]
    fn stemming_a_phrase_key_stems_its_pieces() {
        let base = Vocabulary::shared(VocabularyKind::Afinn, Language::English).unwrap();
        let stemmed = base.stemmed(&verbora_stemmers::PorterStemmer::new());
        assert_eq!(base.get("bad luck").map(Polarity::value), Some(-2.0));
        // `bad` and `luck` are their own stems, so the phrase keeps its form.
        assert_eq!(stemmed.get("bad luck").map(Polarity::value), Some(-2.0));
        assert_eq!(stemmed.get("bad-luck").map(Polarity::value), Some(-2.0));
    }

    #[test]
    fn the_identity_stemmer_changes_no_answer() {
        let base = Vocabulary::shared(VocabularyKind::Pattern, Language::Italian).unwrap();
        let same = base.stemmed(&crate::NoStemmer);
        // `VERO` and `vero` share a lookup form, so the rebuild is allowed to
        // be shorter — but never to answer differently.
        assert!(same.len() <= base.len());
        for key in base.keys() {
            assert_eq!(
                same.get(key).map(Polarity::value),
                base.get(key).map(Polarity::value),
                "{key}"
            );
        }
    }

    #[test]
    fn assignment_keeps_the_original_position() {
        let mut build = Build::new(VocabularyKind::Afinn, 3);
        build.insert(Cow::Borrowed("a"), Forms::of("a"), "1", 1.0);
        build.insert(Cow::Borrowed("b"), Forms::of("b"), "2", 2.0);
        build.insert(Cow::Borrowed("a"), Forms::of("a"), "9", 9.0);
        let v = build.finish();
        assert_eq!(v.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(v.get("a").map(Polarity::value), Some(9.0));
    }

    #[test]
    fn a_later_key_shadows_an_earlier_one_with_the_same_lookup_form() {
        let mut build = Build::new(VocabularyKind::Afinn, 2);
        build.insert(Cow::Borrowed("cover up"), Forms::of("cover up"), "1", 1.0);
        build.insert(Cow::Borrowed("Cover-Up"), Forms::of("Cover-Up"), "9", 9.0);
        let v = build.finish();
        // Both entries are still in the table…
        assert_eq!(v.len(), 2);
        assert_eq!(v.keys().collect::<Vec<_>>(), ["cover up", "Cover-Up"]);
        // …and every spelling resolves to the later one, aliases included.
        for spelling in ["cover up", "cover-up", "Cover-Up", "COVER UP"] {
            assert_eq!(
                v.get(spelling).map(Polarity::value),
                Some(9.0),
                "{spelling}"
            );
        }
        assert_eq!(v.span_from("cover"), 2);
        assert_eq!(v.span_from("up"), 0);
    }
}
