# verbora-normalizers

Rewrites text, and says in each function's name exactly what the rewrite is.
Five of them: `nfc`, `nfd`, `nfkc` and `nfkd` are the four Unicode normalization
forms, and `remove_diacritics` folds combining marks away so that `resume`
matches `résumé`. Reach for it when you need one canonical spelling per abstract
character — for a search key, a hash, a database column or a comparison — and
want the accent-, width- and ligature-insensitivity decisions made explicitly
rather than by whatever your string comparison happened to do.

## Contract

The four forms are [UAX #15](https://www.unicode.org/reports/tr15/) §1.2, and
`remove_diacritics` is defined on top of them as NFD, drop every scalar whose
`Canonical_Combining_Class` is non-zero, NFC — which for Thai and Devanagari
changes the word rather than de-accenting it, so read that function's own table
before using it outside Latin script. The unit is the Unicode scalar value,
because that is the unit UAX #15 defines the forms over; there is no UTF-16
anywhere in the crate, so nothing here can emit `U+FFFD` unless `U+FFFD` was in
the input. Every function returns `Cow::Borrowed` **if and only if** the result
is byte-identical to the input, which makes matching on the `Cow` a correct way
to ask "was this already normalized?" rather than a fast path that might stop
working. Results move with the Unicode Character Database — `unicode_version()`
reports the version in force, a bump is a semver-visible behaviour change, and
anything persisting normalizer-derived keys should stamp it and refuse to load
across a change.

## Example

```rust
use std::borrow::Cow;

use verbora_normalizers::{nfc, nfkc, remove_diacritics};

assert_eq!(nfc("e\u{0301}"), "é");            // compose
assert_eq!(nfkc("ｶﾞ"), "ガ");                 // width- and mark-fold
assert_eq!(remove_diacritics("résumé"), "resume");

// Borrowed exactly when nothing changed — a guarantee, not an optimization.
assert!(matches!(nfc("already composed"), Cow::Borrowed(_)));

// The forms compose, and nothing does it for you: both rewrites stay visible.
assert_eq!(remove_diacritics(&nfkc("Ｃａｆé")), "Cafe");
```

## See also

Full documentation: <https://verbora.dev/features/normalizers>.

This is the one crate in Verbora's text-shaping group whose job *is* rewriting:
[`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers) and
[`verbora-ngrams`](https://crates.io/crates/verbora-ngrams) never alter the text
they are given, so normalize before you tokenize if you want folded tokens. If
what you actually wanted was a *phonetic* key — words that sound alike colliding
rather than words that are spelled alike — that is
[`verbora-phonetics`](https://crates.io/crates/verbora-phonetics), and if you
wanted Japanese kana turned into Latin letters, that is
[`verbora-transliterators`](https://crates.io/crates/verbora-transliterators).
