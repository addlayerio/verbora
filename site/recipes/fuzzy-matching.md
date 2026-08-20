# Fuzzy name matching

Find the records that probably mean the same person, given a misspelled query.

The interesting part is not which distance metric you pick. It is that you must
**not** run a distance metric over every candidate.

## The mistake worth avoiding

```rust  ignore
// O(n) edit distances per query. At 100,000 names this is the whole problem,
// and no amount of making levenshtein() faster fixes it.
let best = names
    .iter()
    .map(|n| (n, levenshtein(query, n)))
    .min_by_key(|&(_, d)| d);
```

`levenshtein/ascii/16` measures 41.8 ns †. Against 100,000 candidates that is
~4 ms of pure comparison per query — roughly 240 queries per second per core,
and it grows linearly with your dictionary. Bucketing does not.

† Pending re-measurement, and left as recorded rather than replaced with a
guess. See [Benchmarks: string distance](../benchmarks/distance.md). The shape
of the argument does not depend on it: the bucketing win is a change of
complexity class, not a constant factor.

## The shape that works

```text
query
  │
  ├─ 1. BUCKET     phonetic key ──▶ a handful of candidates      (O(1) lookup)
  │
  ├─ 2. RANK       edit distance over that handful               (O(bucket))
  │
  └─ 3. THRESHOLD  drop anything below a similarity floor
```

Step 1 does the work. Step 2 makes the answer good.

## Step 1: bucket by phonetic key

<div class="callout callout-note">
<strong>There is a built-in for this step.</strong>
<a href="../features/phonetic-index">Phonetic neighbors</a>
(<code>PhoneticIndex</code>) does exactly what the hand-rolled
<code>HashMap</code> below does, in less code, and it handles
<code>DoubleMetaphone</code>'s two codes per entry for you — reach for it for any
build-once, query-many dictionary. The version below shows what that index does
internally; Step 2 applies unchanged to
<code>PhoneticIndex::neighbors()</code>'s output.
</div>

```rust
use std::collections::HashMap;

use verbora_phonetics::SoundEx;

/// Build once, at startup. Every name is indexed under how it sounds.
fn build_buckets<'a>(names: &[&'a str]) -> HashMap<String, Vec<&'a str>> {
    let soundex = SoundEx::new();
    let mut buckets: HashMap<String, Vec<&str>> = HashMap::new();

    for name in names {
        buckets.entry(soundex.process(name)).or_default().push(name);
    }

    buckets
}

let names = ["Robert", "Rupert", "Rubin", "Ashcraft", "Ashcroft", "Tymczak"];
let buckets = build_buckets(&names);

// "Robert" and "Rupert" collide; "Rubin" does not.
assert_eq!(buckets["R163"], ["Robert", "Rupert"]);
assert_eq!(buckets["R150"], ["Rubin"]);

// Ashcraft and Ashcroft collide, which is the point.
assert_eq!(buckets["A261"], ["Ashcraft", "Ashcroft"]);
```

## Step 2: rank within the bucket

```rust
use std::collections::HashMap;

use verbora_distance::jaro_winkler;
use verbora_phonetics::SoundEx;

fn best_matches<'a>(
    query: &str,
    buckets: &HashMap<String, Vec<&'a str>>,
    limit: usize,
) -> Vec<(&'a str, f64)> {
    let soundex = SoundEx::new();

    let key = soundex.process(query);

    let mut scored: Vec<(&str, f64)> = buckets
        .get(&key)
        .map(|candidates| {
            candidates
                .iter()
                .map(|n| (*n, jaro_winkler(query, n)))
                .collect()
        })
        .unwrap_or_default();

    // Higher is closer for Jaro–Winkler. Check the direction of your metric!
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(limit);
    scored
}
let mut buckets: HashMap<String, Vec<&str>> = HashMap::new();
buckets.insert("R163".to_owned(), vec!["Robert", "Rupert"]);

let hits = best_matches("Robbert", &buckets, 2);

assert_eq!(hits[0].0, "Robert");
assert!(hits[0].1 > 0.96);
assert_eq!(hits[1].0, "Rupert");
```

Two distance calls instead of six. At real scale it is a few dozen instead of a
hundred thousand.

<div class="callout callout-warn">
<strong>Direction is not uniform.</strong> Jaro, Jaro–Winkler and Dice are
<em>similarities</em> — a finite <code>f64</code> in <code>0.0..=1.0</code>,
higher is closer, and identical inputs score exactly <code>1.0</code>.
Levenshtein, Damerau, OSA and Hamming are <em>distances</em> — a
<code>usize</code> count of edits, lower is closer, and identical inputs score
<code>0</code>. Sort a similarity descending and a distance ascending, and never
mix the two in one ranking. Verbora does not fold them into a single convention:
a count and a ratio are different quantities, and normalising would throw away
the exact, <code>Ord</code>, <code>Hash</code> integer the distances return.
</div>

## Choosing the metric for step 2

| Situation | Metric | Why |
|---|---|---|
| Personal names | `jaro_winkler` | Weights a shared prefix; names rarely differ at the front |
| Free-text typos | `levenshtein` | Models insert/delete/substitute directly |
| Typos including swapped letters | `damerau_levenshtein` | Adds transposition — `teh` → `the` is one edit, not two |
| Short codes of equal length | `hamming` | Position-wise; returns `None` when the scalar counts differ, so an incomparable candidate drops out of a `filter_map` instead of scoring |
| Word-order-insensitive overlap | `dice_coefficient` | Bigram set overlap. Case and whitespace are significant — fold both operands first if that is not what you want |

See [Choosing a distance API](../choosing/distance.md).

## Choosing the encoder for step 1

| Encoder | Buckets by | Good for |
|---|---|---|
| `SoundEx` | a letter and 3 digits, English consonant classes | English surnames; wide buckets |
| `Metaphone` | letters, unbounded, English pronunciation rules | General English words; tighter buckets |
| `DoubleMetaphone` | **two** keys of up to 4 characters | Names with more than one plausible pronunciation — index under both |
| `DaitchMokotoff` | 6 digits, branching on ambiguous clusters | Slavic, Germanic and Ashkenazi-Jewish surnames |
| `Nysiis` | letters, US-census name rules | American surnames, where it was designed and evaluated |
| `Cologne` | digits, German phonology | German-language names and words |
| `BeiderMorse` | a candidate list, per language set | Names whose language of origin is itself uncertain |

`DoubleMetaphone` is worth the extra index entry when your data is genuinely
multilingual: a name gets a primary and an alternate key, and a query matching
either finds it. `DaitchMokotoff::codes` goes further and returns *every* branch
an ambiguous cluster produces — index a name under all of them.

Twelve encoders ship in all; the seven above are the ones whose publications
were written for personal names. See [Phonetics](../features/phonetics.md).

## Tuning the recall/precision trade-off

**Buckets too narrow** — the right answer is not in the bucket. Widen by indexing
under more than one key: both Double Metaphone keys, or a phonetic key *and* a
short prefix.

**Buckets too wide** — you are back to scanning. Narrow with a longer key
(`Metaphone` over `SoundEx`), or add a cheap second filter before the distance
call: a length gate rejects most non-matches for the cost of two linear scans
and a subtraction, against an `O(nm)` metric.

```rust
/// Two strings cannot be within `max_edits` if their lengths differ by more.
fn plausible(a: &str, b: &str, max_edits: usize) -> bool {
    a.chars().count().abs_diff(b.chars().count()) <= max_edits
}

assert!(plausible("Robert", "Robbert", 2));
assert!(!plausible("Robert", "Ro", 2));
```

Count scalars, not bytes. `s.chars().count()` is the length every metric here
measures in — one Unicode scalar value is one unit — and the gate is sound
because `|a.chars().count() - b.chars().count()| <= levenshtein(a, b)` holds for
every pair: an insertion or deletion moves the scalar count by exactly one, a
substitution by zero. `str::len()` counts bytes, so a gate built on it starts
rejecting true matches the moment the input leaves ASCII.

## Normalising first

Fold accents and case before indexing *and* before querying, or `José` and `Jose`
land in different buckets:

```rust
use verbora_normalizers::remove_diacritics;

fn normalise(name: &str) -> String {
    remove_diacritics(name).to_lowercase()
}

assert_eq!(normalise("José"), "jose");
assert_eq!(normalise("JOSE"), "jose");
```

`remove_diacritics` returns `Cow`, so unaccented names cost nothing;
`to_lowercase` always allocates, so do this once at index time rather than per
comparison.

## Step 1, an alternative: bucket by edit distance instead of sound

Phonetic bucketing groups *sound-alike* candidates, so it misses a typo that
changes how a name sounds (a transposed letter, a doubled consonant) while
keeping its spelling close. `verbora-spellcheck`'s `FuzzyIndex` covers that case:
same build-then-query shape as `PhoneticIndex`, but it answers "which stored
words are within `k` edits of this query?" using a BK-tree — still fast candidate
generation, not a distance metric run over every entry.

```rust
use verbora_spellcheck::FuzzyIndexBuilder;

let mut builder = FuzzyIndexBuilder::new();
builder.insert_all(["Smith", "Smyth", "Smithe", "Jones"]);
let index = builder.build();

// Each hit carries the exact edit distance the index already computed to
// decide it was a hit, so ranking needs no second pass over a metric.
let mut hits: Vec<(&str, u32)> = index
    .neighbors("Smith", 2)
    .map(|n| (n.word, n.distance))
    .collect();
hits.sort_by_key(|&(word, distance)| (distance, word));

assert_eq!(hits, [("Smith", 0), ("Smithe", 1), ("Smyth", 1)]);
assert!(!hits.iter().any(|&(word, _)| word == "Jones"));
```

`neighbors` is lazy and its order is depth-first from the root, not ranked — sort
by `Neighbor::distance` at the call site, as above, when you want a ranking.

The two bucketing strategies are complementary, not competing: phonetic
bucketing catches "sounds the same, spelled differently"; edit-distance
bucketing catches "spelled almost the same, however it sounds." A caller
who needs both runs Step 2 (ranking) over the union of both indexes'
candidates.

## Bring your own dictionary

`verbora-spellcheck` ships Norvig-style correction and the `FuzzyIndex` above,
but no bundled word list — both take a caller-supplied dictionary.

For prefix-shaped queries rather than sound-alike ones, a
[trie](autocomplete.md) is the better index.

## Related

- [Phonetic neighbors](../features/phonetic-index) — the built-in index behind Step 1
- `FuzzyIndex` (`verbora-spellcheck`) — the edit-distance alternative to Step 1 above
- [Phonetics](../features/phonetics.md) · [String distance](../features/distance.md)
- [Choosing a distance API](../choosing/distance.md)
- [Prefix autocomplete](autocomplete.md)
