# verbora-transliterators

Romanizes Japanese kana into modified-Hepburn romaji: `とうきょう` becomes
`tōkyō`, `ざっし` becomes `zasshi`, `ほんや` becomes `hon'ya`. One left-to-right
pass rewrites kana mora by mora and copies everything else through byte for
byte. It is for sorting, display, search keys and URL slugs over Japanese text —
anywhere the Latin alphabet is what you have to work with.

## Contract

The readings are **modified Hepburn** as codified in the *ALA-LC Romanization
Tables: Japanese* (American Library Association / Library of Congress), which
follows ANSI Z39.11-1972 and BS 4812:1972, extended by 内閣告示第二号
「外来語の表記」 (Cabinet of Japan, Notification No. 2 of 1991) for the syllables
Japanese writes foreign sounds with. The unit is the **mora**, not the byte, the
scalar or the grapheme cluster, and the crate is grapheme-driven with no
dictionary and no notion of a word — three visible consequences are part of the
contract rather than defects: particles are romanized by their kana value
(`こんにちは` is `konnichiha`, not `konnichiwa`), kanji are copied through
unread, and `おう` is always long, so `おもう` is `omō`. It is total (no input
panics), idempotent, and returns `Cow::Borrowed` **if and only if** no mora was
found, which makes matching on the `Cow` a correct way to ask "did this contain
kana?"; nothing is invented, so the output holds no `U+FFFD` unless the input
did. It expects NFC input — decomposed kana, halfwidth katakana and fullwidth
Latin pass through unchanged, and `transliterate_ja_normalized` is the pairing
that folds them first. No performance figures are published: the benchmark suite
exists but has not been run against this implementation.

## Example

```rust
use verbora_transliterators::transliterate_ja;

assert_eq!(transliterate_ja("あいうえお かきくけこ"), "aiueo kakikukeko");
assert_eq!(transliterate_ja("とうきょう"), "tōkyō");

// The sokuon geminates, the syllabic nasal picks its form from what follows.
assert_eq!(transliterate_ja("まっか ざっし たった"), "makka zasshi tatta");
assert_eq!(transliterate_ja("まんと ばんび ほんや"), "manto bambi hon'ya");

// Everything that is not kana is copied through, kanji included.
assert_eq!(transliterate_ja("abc ABC 漢字 (.)"), "abc ABC 漢字 (.)");
```

## See also

Full documentation: <https://verbora.dev/features/transliterators>.

Romanization is not normalization and not a phonetic key. Folding halfwidth
katakana or composing a stray voiced sound mark is Unicode compatibility
normalization — [`verbora-normalizers`](https://crates.io/crates/verbora-normalizers),
which this crate calls into for `transliterate_ja_normalized` rather than
carrying a width table of its own. Making similar-sounding *names* collide is
[`verbora-phonetics`](https://crates.io/crates/verbora-phonetics), and stemming
katakana is `StemmerJa` in
[`verbora-stemmers`](https://crates.io/crates/verbora-stemmers).
