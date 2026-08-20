# verbora-phonetics

Phonetic encoders: functions that turn a word into a key so that
similar-sounding words collide. Twelve algorithms, plus `PhoneticIndex`, a
blocking structure that answers *which of these ten thousand names sound like
this one?* without a linear scan. For name matching, record linkage,
deduplication and search over spellings nobody agrees on.

## The algorithms, and what each cites

Every encoder implements a published algorithm, and cites it — that is what
makes its output checkable rather than asserted.

| Type | Publication |
|---|---|
| `SoundEx` | Russell 1918; NARA, *The Soundex Indexing System* |
| `RefinedSoundex` | **no publication** — see below |
| `Metaphone` | Philips 1990, *Computer Language* 7(12) |
| `DoubleMetaphone` | Philips 2000, *C/C++ Users Journal* 18(6) |
| `DaitchMokotoff` | Daitch–Mokotoff Soundex, Gary Mokotoff and Randy Daitch, 1985 |
| `Nysiis` | Taft 1970, *Name Search Techniques* |
| `Caverphone1` / `Caverphone2` | Hood 2002 / 2004, Caversham Project |
| `Cologne` | Postel 1969, *IBM-Nachrichten* 19 |
| `Phonex` | Lait and Randell 1996 |
| `MatchRatingApproach` | Moore et al. 1977, *Western Union* |
| `BeiderMorse` | Beider and Morse phonetic matching, over their per-language rule corpus |

Refined Soundex is the one row without a paper, and the crate says so rather
than inventing one: no publication, no standards document, no author's
specification exists — the algorithm is the letter-to-digit mapping that
ships in the Apache Commons Codec distribution, which is therefore its
reference definition. `RefinedSoundex`'s own documentation states that
mapping in full.

## The contract

**Every encoder reads one Unicode scalar at a time.** Nothing here indexes
text by byte or by UTF-16 code unit, so no input is split in the middle of a
character and no output contains a character the input did not imply. What an
encoder does with a scalar is its own publication's business: the
Latin-alphabet three (`SoundEx`, `Metaphone`, `DoubleMetaphone`) read `A`–`Z`
after simple ASCII case folding and skip everything else, while the encoders
specified for a language (`Cologne`, `DaitchMokotoff`, `BeiderMorse`) fold the
accented letters their publications name.

**No encoder here fails.** Every `process` is total: no `Result`, no panic, on
any `&str`. Input with no letter the algorithm recognises yields an empty key
— the absence of a key, not a value standing in for one, since no input with
a recognised letter can produce it. `compare` is defined as key equality,
with one deliberate exception: `DoubleMetaphone::compare` matches when the two
names share *either* key, which is the entire reason that algorithm produces
two.

`InlineCode<N>`, the fixed-capacity code used for index keys, stores its
occupied length in one byte, so **`N > 255` is now refused at compile time**
by a `const` assertion in both constructors — `InlineCode<256>` fails to
build rather than wrapping a length and making the documented-as-total
`as_str` panic.

## Example

```rust
use verbora_phonetics::{DoubleMetaphone, Metaphone, PhoneticIndexBuilder, SoundEx};

assert_eq!(SoundEx::new().process("Robert"), "R163");
assert_eq!(Metaphone::new().process("phonetics"), "FNTKS");
assert_eq!(DoubleMetaphone::new().process("Smith").primary(), "SM0");

// The set question: build once, freeze, then query.
let mut builder = PhoneticIndexBuilder::new(SoundEx::new());
builder.insert("Smith");
builder.insert("Smyth");
builder.insert("Johnson");
let index = builder.build();

let neighbors: Vec<&str> = index.neighbors("Smith").collect();
assert!(neighbors.contains(&"Smith"));
assert!(neighbors.contains(&"Smyth")); // same Soundex code
assert!(!neighbors.contains(&"Johnson"));
```

## See also

Full documentation, including which encoder suits which workload:
<https://verbora.dev/features/phonetics>; `PhoneticIndex` in detail at
<https://verbora.dev/features/phonetic-index>; Beider-Morse at
<https://verbora.dev/features/beider-morse>.

To find out *which* of these encoders applies to a given language or script —
and whether a transliteration step belongs in front of it — see
`verbora-language`. For matching spellings rather than sounds, see
`verbora-distance` and `verbora-spellcheck`; to romanize non-Latin script
before encoding, `verbora-transliterators`.
