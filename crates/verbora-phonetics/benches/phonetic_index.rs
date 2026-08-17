// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for [`PhoneticIndex`]: build cost, query cost, and
//! the encode-vs-bucket-lookup ratio that `src/index.rs`'s own module doc
//! comment cites as evidence. Run with:
//!
//! ```text
//! cargo bench -p verbora-phonetics --bench phonetic_index
//! ```
//!
//! # Numbers measured here (machine-dependent, see the run this file's own
//! commit message/PR cites for the exact numbers currently quoted in
//! `src/index.rs`)
//!
//! `src/index.rs` cites this file for two claims: that bucket lookup and
//! neighbor iteration cost approximately nothing next to `encode()` itself
//! at realistic dictionary sizes, and that its compressed-sparse-row layout
//! beats a frozen `HashMap<Code, Box<[EntryId]>>` and a per-code direct
//! table. Both are decided here, not assumed — see each group's own doc
//! comment for the specific numbers and verdicts:
//!
//! * [`bench_build`] — comparison 1: `PhoneticIndexBuilder::insert` +
//!   `build()`, SoundEx and DoubleMetaphone, 1K/10K/100K entries.
//! * [`bench_neighbors`] — comparison 2: `PhoneticIndex::neighbors()`
//!   latency for a hit, a miss, and a high-cardinality bucket, at each size.
//! * [`bench_neighbors`] + [`bench_encode_only`] together — comparison 3,
//!   the critical one: `encode()` alone vs. the full `neighbors()` call.
//! * [`bench_alt_designs_build`] + [`bench_alt_designs_query`] — comparisons
//!   4-6: `InlineCode` + CSR vs. a plain `HashMap<String, Vec<EntryId>>`
//!   (comparison 4), vs. a frozen `HashMap<Code, Box<[EntryId]>>`
//!   (comparison 5), and — SoundEx only, since its code space is genuinely
//!   small — vs. a dense array indexed by a perfect hash of the code
//!   (comparison 6).
//!
//! # Dictionary
//!
//! [`build_dictionary`] mixes the workspace's shared synthetic word list
//! (`benches/data/words.json`, the same input `benches/phonetics.rs` uses)
//! with a pool of 50 real, common English surnames, so codes cluster the
//! way a real name dictionary's codes do — a handful of common codes with
//! large buckets, a long tail of singletons — rather than spreading
//! uniformly across the code space the way pure random filler would. Three
//! canary queries are inserted at fixed, size-independent counts so every
//! run has a real hit, a real miss, and a real high-cardinality bucket to
//! query: see [`HIT_QUERY`], [`MISS_QUERY`], [`HIGH_CARDINALITY_QUERY`].
//! [`assert_scenarios_are_honest`] verifies all three hold for the actual
//! built index before any benchmark runs, so a canary that stops being
//! accurate (e.g. because the corpus changes) fails loudly instead of
//! silently mislabeling a group.

use std::collections::HashMap;
use std::hint::black_box;
use std::time::Duration;

use criterion::measurement::Measurement;
use criterion::{
    BenchmarkGroup, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use verbora_phonetics::{
    DoubleMetaphone, PhoneticCodes, PhoneticEncoder, PhoneticIndex, PhoneticIndexBuilder, SoundEx,
    SoundexCode,
};

/// Index sizes exercised by comparisons 1 and 2: below a typical dictionary,
/// at a typical one, and a large one — the same bracket
/// `benches/phonetics.rs`'s own `parallel_batch` group uses for the same
/// reason (see that file's doc comment).
const SIZES: [usize; 3] = [1_000, 10_000, 100_000];

/// The dictionary size comparisons 4-6 run at, per this bench file's own
/// brief: "at 100K entries".
const ALT_DESIGN_SIZE: usize = 100_000;

/// Fifty real, common English surnames (the most frequent US Census
/// surnames), sprinkled into the synthetic filler so phonetic codes cluster
/// the way a real name dictionary's do: a handful of large buckets (Smith,
/// Johnson, Williams, ...) and a long tail of singletons, not a uniform
/// spread across the code space.
const SURNAME_POOL: [&str; 50] = [
    "Smith",
    "Johnson",
    "Williams",
    "Brown",
    "Jones",
    "Garcia",
    "Miller",
    "Davis",
    "Rodriguez",
    "Martinez",
    "Hernandez",
    "Lopez",
    "Gonzalez",
    "Wilson",
    "Anderson",
    "Thomas",
    "Taylor",
    "Moore",
    "Jackson",
    "Martin",
    "Lee",
    "Perez",
    "Thompson",
    "White",
    "Harris",
    "Sanchez",
    "Clark",
    "Ramirez",
    "Lewis",
    "Robinson",
    "Walker",
    "Young",
    "Allen",
    "King",
    "Wright",
    "Scott",
    "Torres",
    "Nguyen",
    "Hill",
    "Flores",
    "Green",
    "Adams",
    "Nelson",
    "Baker",
    "Hall",
    "Rivera",
    "Campbell",
    "Mitchell",
    "Carter",
    "Roberts",
];

/// The "hit" query: distinctive enough that it cannot collide with the
/// filler or the surname pool, inserted exactly once so every size has
/// exactly one exact-text match plus whatever else shares its code.
const HIT_QUERY: &str = "Featherstonehaugh";

/// The "high-cardinality bucket" query: a genuinely common English surname.
/// Inserted [`high_cardinality_count`] times on top of its share of the
/// surname-pool sprinkle, so its own bucket is large at every size.
const HIGH_CARDINALITY_QUERY: &str = "Smith";

/// The "miss" query. SoundEx's code space is coarse enough (one letter plus
/// three digits drawn from a 7-value alphabet, since SoundEx never assigns
/// 7/8/9) that most consonant-heavy nonsense strings collide with *some*
/// code already present in a 100K-entry dictionary — a brute-force search
/// over every four-letter string was needed to find one that misses both
/// encoders at every size used below. [`assert_scenarios_are_honest`]
/// verifies that is still true against the real built index before any
/// benchmark runs — this is not an assumption, it is checked every time
/// this file runs.
const MISS_QUERY: &str = "Abfr";

/// How many extra copies of [`HIGH_CARDINALITY_QUERY`] to insert on top of
/// its share of the surname-pool sprinkle, so the high-cardinality bucket
/// scales with dictionary size instead of staying fixed.
fn high_cardinality_count(n: usize) -> usize {
    (n / 100).max(50)
}

/// Reads the workspace's shared synthetic word list, the same one
/// `benches/phonetics.rs` uses — failing loudly if it has not been
/// generated, matching that file's own convention exactly.
fn words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is two levels below the workspace root")
        .join("benches/data/words.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    json["words"]
        .as_array()
        .expect("words array")
        .iter()
        .map(|w| w.as_str().expect("word is a string").to_owned())
        .collect()
}

/// Builds an `n`-entry realistic dictionary: mostly the shared synthetic
/// word list (cycled once it runs out, like `benches/phonetics.rs`'s own
/// `bench_parallel_batch` does), with every seventh filler slot replaced by
/// a surname from [`SURNAME_POOL`] so codes cluster realistically, plus the
/// three fixed canaries appended at the end. See this file's own module doc
/// comment for why.
fn build_dictionary(n: usize) -> Vec<String> {
    let corpus = words();
    let high_card = high_cardinality_count(n);
    let filler_needed = n
        .checked_sub(high_card + 1)
        .expect("dictionary size too small for the fixed canaries");

    let mut dict = Vec::with_capacity(n);
    for i in 0..filler_needed {
        if i % 7 == 0 {
            dict.push(SURNAME_POOL[(i / 7) % SURNAME_POOL.len()].to_owned());
        } else {
            dict.push(corpus[i % corpus.len()].clone());
        }
    }
    dict.push(HIT_QUERY.to_owned());
    dict.extend(std::iter::repeat_n(
        HIGH_CARDINALITY_QUERY.to_owned(),
        high_card,
    ));
    dict
}

/// Verifies the three canary queries behave as advertised against the
/// *actual* built index, before any benchmark runs — a hit produces at
/// least one neighbor, a miss produces exactly zero, and the
/// high-cardinality query produces meaningfully more than a hit would.
/// Panics with the offending label if a canary stops being accurate (e.g.
/// after a corpus or dictionary-composition change), so a mislabeled group
/// fails loudly instead of quietly reporting the wrong thing as a "miss".
fn assert_scenarios_are_honest<E: PhoneticEncoder>(idx: &PhoneticIndex<E>, label: &str) {
    let hit = idx.neighbors(HIT_QUERY).count();
    assert!(hit >= 1, "{label}: HIT_QUERY produced no neighbors");

    let miss = idx.neighbors(MISS_QUERY).count();
    assert_eq!(
        miss, 0,
        "{label}: MISS_QUERY unexpectedly matched {miss} entries"
    );

    let high_card = idx.neighbors(HIGH_CARDINALITY_QUERY).count();
    assert!(
        high_card > hit,
        "{label}: HIGH_CARDINALITY_QUERY ({high_card}) is not meaningfully larger than a hit ({hit})"
    );
}

/// Applies a uniform, reduced sampling budget to every group in this file.
/// Precise confidence intervals are not the point here — the point is real,
/// reproducible orders of magnitude across ~46 benchmark points without an
/// unreasonable total run time. `sample_size(30)` is Criterion's own
/// documented minimum-recommended size, not an arbitrary cut.
fn configure<M: Measurement>(g: &mut BenchmarkGroup<'_, M>) {
    g.sample_size(30);
    g.measurement_time(Duration::from_secs(2));
    g.warm_up_time(Duration::from_millis(500));
}

// ---------------------------------------------------------------------------
// Comparison 1: build cost
// ---------------------------------------------------------------------------

/// `PhoneticIndexBuilder::insert` + `build()`, SoundEx (single-code) and
/// DoubleMetaphone (dual-code), at 1K/10K/100K entries.
fn bench_build(c: &mut Criterion) {
    let mut g = c.benchmark_group("build");
    configure(&mut g);

    for &n in &SIZES {
        let dict = build_dictionary(n);
        g.throughput(Throughput::Elements(n as u64));

        g.bench_with_input(BenchmarkId::new("soundex", n), &dict, |b, dict| {
            b.iter(|| {
                let mut builder = PhoneticIndexBuilder::new(SoundEx::new());
                for w in black_box(dict) {
                    builder.insert(w);
                }
                builder.build()
            });
        });

        g.bench_with_input(BenchmarkId::new("double_metaphone", n), &dict, |b, dict| {
            b.iter(|| {
                let mut builder = PhoneticIndexBuilder::new(DoubleMetaphone::new());
                for w in black_box(dict) {
                    builder.insert(w);
                }
                builder.build()
            });
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// Comparison 2 (+ half of comparison 3): query cost
// ---------------------------------------------------------------------------

/// `PhoneticIndex::neighbors()`, fully drained with `.count()` so laziness
/// does not let Criterion skip the real work: a hit, a miss, and the
/// high-cardinality bucket, for both encoders, at each size.
///
/// Paired with [`bench_encode_only`]'s numbers, this is also half of
/// comparison 3 — the full `neighbors()` cost here minus the `encode()`-only
/// cost there is (approximately) the cost of bucket lookup and iteration
/// alone.
fn bench_neighbors(c: &mut Criterion) {
    let mut g = c.benchmark_group("neighbors");
    configure(&mut g);

    for &n in &SIZES {
        let dict = build_dictionary(n);

        let mut sx_builder = PhoneticIndexBuilder::new(SoundEx::new());
        sx_builder.extend(dict.iter().map(String::as_str));
        let sx_index = sx_builder.build();

        let mut dm_builder = PhoneticIndexBuilder::new(DoubleMetaphone::new());
        dm_builder.extend(dict.iter().map(String::as_str));
        let dm_index = dm_builder.build();

        assert_scenarios_are_honest(&sx_index, &format!("soundex@{n}"));
        assert_scenarios_are_honest(&dm_index, &format!("double_metaphone@{n}"));

        g.throughput(Throughput::Elements(1));

        for (scenario, query) in [
            ("hit", HIT_QUERY),
            ("miss", MISS_QUERY),
            ("high_cardinality", HIGH_CARDINALITY_QUERY),
        ] {
            g.bench_with_input(
                BenchmarkId::new(format!("soundex_{scenario}"), n),
                &query,
                |b, &query| b.iter(|| sx_index.neighbors(black_box(query)).count()),
            );
            g.bench_with_input(
                BenchmarkId::new(format!("double_metaphone_{scenario}"), n),
                &query,
                |b, &query| b.iter(|| dm_index.neighbors(black_box(query)).count()),
            );
        }
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// Comparison 3 (other half): encode() alone
// ---------------------------------------------------------------------------

/// `PhoneticEncoder::encode()` alone, independent of any index — the
/// baseline comparison 3 measures `neighbors()` against. Index size is
/// irrelevant to this cost (encoding a query string never touches the
/// index), so this runs once per encoder/scenario rather than once per
/// size.
fn bench_encode_only(c: &mut Criterion) {
    let soundex = SoundEx::new();
    let dm = DoubleMetaphone::new();

    let mut g = c.benchmark_group("encode_only");
    configure(&mut g);

    for (scenario, query) in [
        ("hit", HIT_QUERY),
        ("miss", MISS_QUERY),
        ("high_cardinality", HIGH_CARDINALITY_QUERY),
    ] {
        g.bench_function(format!("soundex_{scenario}"), |b| {
            b.iter(|| PhoneticEncoder::encode(&soundex, black_box(query)));
        });
        g.bench_function(format!("double_metaphone_{scenario}"), |b| {
            b.iter(|| PhoneticEncoder::encode(&dm, black_box(query)));
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// Comparisons 4-6: throwaway alternative designs
//
// None of the three structs below ship. They exist only so the shipped
// `InlineCode` + compressed-sparse-row design in `src/index.rs` has a real
// number to beat, per comparisons 4-6 in this crate's benchmarking brief.
// ---------------------------------------------------------------------------

/// Comparison 4's alternative: a plain `String`-keyed `HashMap`, built the
/// way a first draft of this index would be.
struct StringMapIndex {
    entries: Vec<String>,
    buckets: HashMap<String, Vec<u32>>,
}

impl StringMapIndex {
    fn build(encoder: &SoundEx, words: &[String]) -> Self {
        let mut entries = Vec::with_capacity(words.len());
        let mut buckets: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, w) in words.iter().enumerate() {
            entries.push(w.clone());
            buckets
                .entry(encoder.process(w))
                .or_default()
                .push(i as u32);
        }
        Self { entries, buckets }
    }

    fn neighbors<'a>(&'a self, encoder: &SoundEx, query: &str) -> impl Iterator<Item = &'a str> {
        let code = encoder.process(query);
        self.buckets
            .get(&code)
            .into_iter()
            .flatten()
            .map(move |&id| self.entries[id as usize].as_str())
    }

    /// Manual memory estimate (real `size_of` measurements, not guesses,
    /// but *not* a live allocator trace — this workspace denies `unsafe`
    /// code, which a counting global allocator would require). Undercounts
    /// slightly: `Vec`/`HashMap` spare capacity beyond `len` is not visible
    /// from outside, malloc's own per-allocation bookkeeping is not
    /// counted, and `hashbrown`'s control-byte array is approximated at one
    /// byte per slot at capacity rather than measured directly.
    fn estimated_bytes(&self) -> usize {
        let entries_spine = self.entries.len() * std::mem::size_of::<String>();
        let entries_chars: usize = self.entries.iter().map(String::len).sum();
        let bucket_key_chars: usize = self.buckets.keys().map(String::len).sum();
        let bucket_id_storage: usize = self
            .buckets
            .values()
            .map(|v| v.len() * std::mem::size_of::<u32>())
            .sum();
        // hashbrown stores the (K, V) pair inline per slot, plus one
        // control byte per slot, sized at the table's own capacity.
        let bucket_slots =
            self.buckets.capacity() * (std::mem::size_of::<(String, Vec<u32>)>() + 1);
        entries_spine + entries_chars + bucket_key_chars + bucket_id_storage + bucket_slots
    }
}

/// Comparison 5's alternative: a frozen `HashMap<Code, Box<[EntryId]>>` —
/// the same immutable, build-once/query-many shape as the shipped index,
/// but hash-keyed instead of sorted-and-offset.
struct FrozenHashIndex {
    entries: Box<[Box<str>]>,
    buckets: HashMap<SoundexCode, Box<[u32]>>,
}

impl FrozenHashIndex {
    fn build(encoder: &SoundEx, words: &[String]) -> Self {
        let entries: Box<[Box<str>]> = words.iter().map(|w| Box::from(w.as_str())).collect();
        let mut staging: HashMap<SoundexCode, Vec<u32>> = HashMap::new();
        for (i, w) in words.iter().enumerate() {
            let PhoneticCodes::One(code) = PhoneticEncoder::encode(encoder, w) else {
                unreachable!("SoundEx is single-code")
            };
            staging.entry(code).or_default().push(i as u32);
        }
        let buckets = staging
            .into_iter()
            .map(|(k, v)| (k, v.into_boxed_slice()))
            .collect();
        Self { entries, buckets }
    }

    fn neighbors<'a>(&'a self, encoder: &SoundEx, query: &str) -> &'a [u32] {
        let PhoneticCodes::One(code) = PhoneticEncoder::encode(encoder, query) else {
            unreachable!("SoundEx is single-code")
        };
        self.buckets.get(&code).map_or(&[][..], |ids| &ids[..])
    }

    /// See [`StringMapIndex::estimated_bytes`] for the same caveats; this
    /// design has no key-string heap allocation to count, only the code's
    /// inline bytes (already counted in the slot's own `size_of`).
    fn estimated_bytes(&self) -> usize {
        let entries_spine = self.entries.len() * std::mem::size_of::<Box<str>>();
        let entries_chars: usize = self.entries.iter().map(|s| s.len()).sum();
        let bucket_ids: usize = self
            .buckets
            .values()
            .map(|v| v.len() * std::mem::size_of::<u32>())
            .sum();
        let bucket_slots =
            self.buckets.capacity() * (std::mem::size_of::<(SoundexCode, Box<[u32]>)>() + 1);
        entries_spine + entries_chars + bucket_ids + bucket_slots
    }
}

/// Comparison 6's alternative, SoundEx-only: a dense array indexed by a
/// perfect hash of the *ASCII* SoundEx code shape (one uppercase letter
/// plus three ASCII digits => 26 * 1000 = 26,000 slots).
///
/// Deliberately narrower than the shipped design: [`SoundexCode`]'s own doc
/// comment documents Unicode inputs that do not fit this 4-byte ASCII
/// shape at all (`"日000"`, `"SS000"`, ...); this harness has no fallback
/// for them, which is itself part of what comparison 6 evaluates — a real
/// SoundEx-specific fast path would need one, and that is exactly the
/// "meaningful complexity" the crate's benchmarking brief asks to weigh
/// against any speed gain.
struct DenseSoundexIndex {
    entries: Box<[Box<str>]>,
    table: Vec<Box<[u32]>>,
}

/// 26 initial letters * 1,000 three-digit suffixes.
const DENSE_SLOTS: usize = 26 * 1000;

/// The perfect hash: `None` for any code outside the ASCII letter+3-digit
/// shape (see [`DenseSoundexIndex`]'s own doc comment).
fn dense_hash(code: SoundexCode) -> Option<usize> {
    let s = code.as_str().as_bytes();
    let [letter, d1, d2, d3] = s else { return None };
    if !letter.is_ascii_uppercase() || ![d1, d2, d3].iter().all(|d| d.is_ascii_digit()) {
        return None;
    }
    let letter = usize::from(letter - b'A');
    let digits =
        usize::from(d1 - b'0') * 100 + usize::from(d2 - b'0') * 10 + usize::from(d3 - b'0');
    Some(letter * 1000 + digits)
}

impl DenseSoundexIndex {
    fn build(encoder: &SoundEx, words: &[String]) -> Self {
        let entries: Box<[Box<str>]> = words.iter().map(|w| Box::from(w.as_str())).collect();
        let mut staging: Vec<Vec<u32>> = (0..DENSE_SLOTS).map(|_| Vec::new()).collect();
        for (i, w) in words.iter().enumerate() {
            let PhoneticCodes::One(code) = PhoneticEncoder::encode(encoder, w) else {
                unreachable!("SoundEx is single-code")
            };
            let slot = dense_hash(code)
                .expect("this benchmark's dictionary is ASCII-only by construction");
            staging[slot].push(i as u32);
        }
        let table = staging.into_iter().map(Vec::into_boxed_slice).collect();
        Self { entries, table }
    }

    fn neighbors<'a>(&'a self, encoder: &SoundEx, query: &str) -> &'a [u32] {
        let PhoneticCodes::One(code) = PhoneticEncoder::encode(encoder, query) else {
            unreachable!("SoundEx is single-code")
        };
        dense_hash(code).map_or(&[][..], |slot| &self.table[slot][..])
    }

    /// Unlike the other two alternatives, this table's spine cost is fixed
    /// (26,000 slots) regardless of dictionary size — the number this
    /// method reports is the whole point of comparison 6's "fixed overhead
    /// vs. marginal gain" question.
    fn estimated_bytes(&self) -> usize {
        let entries_spine = self.entries.len() * std::mem::size_of::<Box<str>>();
        let entries_chars: usize = self.entries.iter().map(|s| s.len()).sum();
        let table_spine = self.table.len() * std::mem::size_of::<Box<[u32]>>();
        let table_ids: usize = self
            .table
            .iter()
            .map(|b| b.len() * std::mem::size_of::<u32>())
            .sum();
        entries_spine + entries_chars + table_spine + table_ids
    }
}

/// The shipped design's own memory cost, computed analytically from real
/// `size_of` measurements of its public types ([`SoundexCode`],
/// [`MetaphoneCode`]) plus the exact row/code counts this bench's own
/// dictionary produces — not a guess, and not a "rough" estimate the way
/// the `HashMap`-based alternatives' are: `PhoneticIndex`'s three arrays
/// are `Box<[T]>` (no spare capacity to omit) and the shipped design has no
/// per-code heap allocation to approximate.
fn shipped_estimated_bytes<E: PhoneticEncoder>(
    encoder: &E,
    words: &[String],
    codes_per_entry: usize,
) -> usize {
    let entries_spine = words.len() * std::mem::size_of::<Box<str>>();
    let entries_chars: usize = words.iter().map(String::len).sum();

    let mut codes: Vec<E::Code> = Vec::with_capacity(words.len() * codes_per_entry);
    for w in words {
        codes.extend(encoder.encode(w));
    }
    codes.sort_unstable();
    codes.dedup();
    let distinct_codes = codes.len();

    let codes_arr = distinct_codes * std::mem::size_of::<E::Code>();
    let offsets_arr = (distinct_codes + 1) * std::mem::size_of::<u32>();
    let ids_arr = words.len() * codes_per_entry * std::mem::size_of::<u32>();

    entries_spine + entries_chars + codes_arr + offsets_arr + ids_arr
}

/// Build time: comparison 4 (`StringMapIndex`) plus, for completeness,
/// comparisons 5 and 6's alternatives too, all at [`ALT_DESIGN_SIZE`].
fn bench_alt_designs_build(c: &mut Criterion) {
    let dict = build_dictionary(ALT_DESIGN_SIZE);
    let soundex = SoundEx::new();

    let mut g = c.benchmark_group("alt_designs_build");
    configure(&mut g);
    g.throughput(Throughput::Elements(ALT_DESIGN_SIZE as u64));

    g.bench_function("inline_csr", |b| {
        b.iter(|| {
            let mut builder = PhoneticIndexBuilder::new(soundex);
            for w in black_box(&dict) {
                builder.insert(w);
            }
            builder.build()
        });
    });
    g.bench_function("string_hashmap", |b| {
        b.iter(|| StringMapIndex::build(&soundex, black_box(&dict)));
    });
    g.bench_function("frozen_hashmap", |b| {
        b.iter(|| FrozenHashIndex::build(&soundex, black_box(&dict)));
    });
    g.bench_function("dense_array", |b| {
        b.iter(|| DenseSoundexIndex::build(&soundex, black_box(&dict)));
    });

    g.finish();
}

/// Query latency for all four SoundEx bucket-storage designs side by side —
/// comparisons 4, 5 and 6's query half — plus a printed memory report for
/// all four, at [`ALT_DESIGN_SIZE`]. Each alternative harness is checked
/// against the shipped index's own neighbor count for every scenario before
/// any benchmark runs, so a bug in a throwaway harness fails loudly instead
/// of quietly producing a misleading number.
fn bench_alt_designs_query(c: &mut Criterion) {
    let dict = build_dictionary(ALT_DESIGN_SIZE);
    let soundex = SoundEx::new();

    let mut sx_builder = PhoneticIndexBuilder::new(soundex);
    sx_builder.extend(dict.iter().map(String::as_str));
    let inline_index = sx_builder.build();
    assert_scenarios_are_honest(&inline_index, &format!("soundex@{ALT_DESIGN_SIZE}"));

    let string_index = StringMapIndex::build(&soundex, &dict);
    let hash_index = FrozenHashIndex::build(&soundex, &dict);
    let dense_index = DenseSoundexIndex::build(&soundex, &dict);

    for (label, query) in [
        ("hit", HIT_QUERY),
        ("miss", MISS_QUERY),
        ("high_cardinality", HIGH_CARDINALITY_QUERY),
    ] {
        let want = inline_index.neighbors(query).count();
        assert_eq!(
            string_index.neighbors(&soundex, query).count(),
            want,
            "StringMapIndex disagrees with the shipped index on {label} -- bug in the harness"
        );
        assert_eq!(
            hash_index.neighbors(&soundex, query).len(),
            want,
            "FrozenHashIndex disagrees with the shipped index on {label} -- bug in the harness"
        );
        assert_eq!(
            dense_index.neighbors(&soundex, query).len(),
            want,
            "DenseSoundexIndex disagrees with the shipped index on {label} -- bug in the harness"
        );
    }

    let inline_bytes = shipped_estimated_bytes(&soundex, &dict, 1);
    let string_bytes = string_index.estimated_bytes();
    let hash_bytes = hash_index.estimated_bytes();
    let dense_bytes = dense_index.estimated_bytes();
    eprintln!(
        "\n[memory @ {ALT_DESIGN_SIZE} entries, SoundEx]\n\
         \x20 inline_csr      = {inline_bytes:>10} bytes ({:.2} B/entry)\n\
         \x20 string_hashmap  = {string_bytes:>10} bytes ({:.2} B/entry, {:.2}x)\n\
         \x20 frozen_hashmap  = {hash_bytes:>10} bytes ({:.2} B/entry, {:.2}x)\n\
         \x20 dense_array     = {dense_bytes:>10} bytes ({:.2} B/entry, {:.2}x)\n",
        inline_bytes as f64 / ALT_DESIGN_SIZE as f64,
        string_bytes as f64 / ALT_DESIGN_SIZE as f64,
        string_bytes as f64 / inline_bytes as f64,
        hash_bytes as f64 / ALT_DESIGN_SIZE as f64,
        hash_bytes as f64 / inline_bytes as f64,
        dense_bytes as f64 / ALT_DESIGN_SIZE as f64,
        dense_bytes as f64 / inline_bytes as f64,
    );

    let mut g = c.benchmark_group("alt_designs_query");
    configure(&mut g);

    for (scenario, query) in [
        ("hit", HIT_QUERY),
        ("miss", MISS_QUERY),
        ("high_cardinality", HIGH_CARDINALITY_QUERY),
    ] {
        g.bench_function(format!("inline_csr_{scenario}"), |b| {
            b.iter(|| inline_index.neighbors(black_box(query)).count());
        });
        g.bench_function(format!("string_hashmap_{scenario}"), |b| {
            b.iter(|| string_index.neighbors(&soundex, black_box(query)).count());
        });
        g.bench_function(format!("frozen_hashmap_{scenario}"), |b| {
            b.iter(|| hash_index.neighbors(&soundex, black_box(query)).len());
        });
        g.bench_function(format!("dense_array_{scenario}"), |b| {
            b.iter(|| dense_index.neighbors(&soundex, black_box(query)).len());
        });
    }

    g.finish();
}

criterion_group!(
    base_benches,
    bench_build,
    bench_neighbors,
    bench_encode_only,
    bench_alt_designs_build,
    bench_alt_designs_query,
);
criterion_main!(base_benches);
