==================================================
EXPANDED SPECIALIZED COMPETITOR SET
==================================================

The current competitor list is NOT considered exhaustive.

Add specialized performance-oriented Rust implementations that may represent
a stronger performance target than the competitors already present in the
benchmark suite.

At minimum, investigate and benchmark the following additional competitors:

DISTANCE
- triple_accel

TRIE / PREFIX STRUCTURES
- fast_radix_trie
- fst, where its frozen/query-oriented semantics can be made exactly comparable

SPELLCHECK / FUZZY DICTIONARY
- fast_symspell
- fst, where Levenshtein automata / fuzzy dictionary search can be made
  semantically equivalent

STEMMERS
- snowball_stemmers_rs

These libraries are ADDITIONS to the existing competitors.

Do NOT remove:

- rapidfuzz
- strsim
- qp-trie
- rust-stemmers
- symspell
- harper-core
- Tantivy
- rphonetic
- whichlang
- lingua
- smartcore
- linfa
- or any other existing relevant benchmark competitor.

The objective is to expand the competitive field.

==================================================
FASTEST IMPLEMENTATION DEFINES THE TARGET
==================================================

For every capability:

benchmark ALL fair relevant competitors.

Then compute:

TARGET = min(
    competitor_1,
    competitor_2,
    competitor_3,
    ...
)

Verbora must then satisfy:

Verbora < TARGET

with a reproducible statistically defensible advantage.

Example:

Levenshtein:

strsim         200 ns
rapidfuzz       90 ns
triple_accel    65 ns
Verbora         70 ns

Verbora beats two competitors.

This is still:

RED

because triple_accel remains faster.

The optimization loop continues until:

Verbora < 65 ns

with sufficient reproducible headroom.

==================================================
FAST_RADIX_TRIE — HARD CHALLENGER
==================================================

Add fast_radix_trie to the Trie competitive investigation.

Compare it against:

Verbora
qp-trie
trie-rs
fast_radix_trie

where operations can be made semantically equivalent.

Investigate particularly:

- contains / exact lookup;
- prefix lookup;
- predictive enumeration;
- iteration;
- construction;
- frozen/read-heavy workloads;
- memory usage.

Study fast_radix_trie's implementation architecture, including where relevant:

- radix/path compression;
- compressed edges;
- node layout;
- child representation;
- byte comparison;
- memchr-style scanning;
- cache locality;
- allocation strategy;
- prefix compression.

Do NOT merely add it to the benchmark.

If it beats Verbora:

study WHY.

Then evaluate whether Verbora should adopt an equivalent or superior
architectural technique while preserving Verbora semantics.

==================================================
TRIE — MULTIPLE REPRESENTATIONS ARE ALLOWED
==================================================

Do not assume Verbora must use the same internal Trie representation for:

BUILD

and:

QUERY.

Verbora already follows the philosophy:

Build
↓
Freeze
↓
Query

Therefore explicitly investigate whether:

mutable/build representation

can be transformed during freeze into:

compressed query representation.

Potential designs to evaluate include:

- arena trie;
- radix trie;
- PATRICIA-style compression;
- compressed paths;
- integer node IDs;
- compact child ranges;
- sorted edge arrays;
- hybrid hash/build + compact/frozen representation.

The public API does not dictate the internal representation.

The winning measured design does.

==================================================
FST — SPECIALIZED FROZEN COMPETITOR
==================================================

Evaluate the `fst` crate as a specialized competitor/reference architecture.

Do NOT automatically classify it as EXACT.

FST represents a different architectural model and may not be directly
comparable to a mutable Trie.

Evaluate it specifically for:

- frozen dictionary lookup;
- prefix-oriented lookup;
- large dictionaries;
- mmap-backed dictionaries;
- compact dictionary representation;
- fuzzy dictionary traversal;
- Levenshtein automata.

If an operation can be made semantically equivalent:

classify it EXACT or NARROWED_EXACT

and make it a GREEN hard gate.

Otherwise classify:

TECHNIQUE

and use it as architectural evidence.

Even when it is TECHNIQUE:

study whether Verbora can borrow the relevant ideas.

==================================================
FAST_SYMSPELL — HARD SPELLCHECK CHALLENGER
==================================================

Add fast_symspell to the Spellcheck competitive suite.

Compare:

Verbora
symspell
fast_symspell
harper-core

for all semantically equivalent operations.

Investigate separately:

BUILD
LOOKUP
CORRECTION distance=1
CORRECTION distance=2
large dictionaries
small dictionaries
batch correction

Do not average these together.

Each exact scenario is an independent GREEN gate.

==================================================
FAST_SYMSPELL ARCHITECTURE REVIEW
==================================================

Study fast_symspell beyond its public API.

Investigate its use, where applicable, of techniques such as:

- precomputed deletion indexes;
- compact archived data;
- rkyv;
- mmap / memmap2;
- specialized hashing;
- triple_accel;
- zero-copy loading;
- frozen structures;
- query-oriented layouts.

This is particularly important because these techniques overlap strongly
with Verbora's performance philosophy.

Do not copy the implementation blindly.

Determine:

WHY does fast_symspell win?

Then ask:

Can Verbora achieve the same benefit with:

less memory
OR
less initialization
OR
faster query
OR
better generality?

The objective is not to reproduce fast_symspell.

The objective is to outperform it.

==================================================
SPELLCHECK MAY REQUIRE AN ARCHITECTURAL CHANGE
==================================================

The existing competitive results indicate that Verbora can have extremely
fast construction while losing heavily in correction generation.

Do not protect cheap construction at the expense of catastrophic query
performance.

Evaluate the complete lifecycle:

build time
memory
serialized size
load time
query latency
batch throughput

Potential architectures include:

- deletion dictionary;
- compact delete index;
- BK-tree;
- FST;
- Levenshtein automaton;
- phonetic blocking;
- hybrid indexes;
- distance-specialized candidate generation.

If necessary, consider:

Builder
↓
Freeze
↓
Query-optimized index

The default implementation should target realistic NLP workloads rather than
winning an isolated construction benchmark.

==================================================
SNOWBALL_STEMMERS_RS — STEMMER CHALLENGER
==================================================

Add snowball_stemmers_rs to the stemmer competitive suite.

Compare, per supported language:

Verbora
rust-stemmers
snowball_stemmers_rs

Only use exact semantic comparisons as GREEN hard gates.

Do NOT assume rust-stemmers remains the fastest simply because it was the
existing benchmark competitor.

For every language:

FASTEST_LANGUAGE_COMPETITOR =
    min(
        rust_stemmers,
        snowball_stemmers_rs,
        any other exact fair implementation
    )

Verbora must beat FASTEST_LANGUAGE_COMPETITOR.

Do this independently for:

English
Spanish
French
Italian
German
Dutch
Norwegian
Portuguese
Russian
Swedish
and every other semantically comparable supported language.

A win in one language does not compensate for a loss in another.

==================================================
STEMMER GENERATED-CODE ANALYSIS
==================================================

snowball_stemmers_rs may provide an additional useful implementation reference
because its algorithms are generated from Snowball definitions.

Compare generated implementation strategies:

Verbora
vs
rust-stemmers
vs
snowball_stemmers_rs.

Inspect:

- state representation;
- suffix matching;
- generated branches;
- buffer mutations;
- UTF handling;
- temporary storage;
- snapshots;
- allocation sites;
- rule dispatch;
- generated code size;
- inlining;
- compiler output.

If a generated implementation beats Verbora:

determine the structural reason.

Do not merely hand-optimize individual language rules if a better code
generation strategy can improve all stemmers.

==================================================
COMPETITOR DISCOVERY IS NOW PART OF THE PROCESS
==================================================

Do NOT treat the competitor list in this prompt as permanently complete.

Before optimizing each capability:

perform a competitor-discovery pass.

Search:

- crates.io;
- GitHub;
- docs.rs;
- Rust NLP ecosystem;
- Rust search/indexing ecosystem;
- Rust SIMD/performance ecosystem;
- specialized algorithm crates.

Look specifically for implementations advertising or demonstrating:

- SIMD;
- bit-parallel algorithms;
- zero-copy;
- mmap;
- rkyv;
- FST;
- radix structures;
- perfect hashing;
- specialized allocators;
- cache-optimized layouts;
- vectorization;
- architecture-specific acceleration;
- generated algorithms.

The question is:

"Is there a specialized Rust implementation faster than the competitors we
currently benchmark?"

If YES:

add it to the investigation.

==================================================
DO NOT TRUST MARKETING BENCHMARKS
==================================================

A competitor claiming:

"fastest"
"SIMD accelerated"
"10x faster"
"zero-copy"

does NOT automatically make it a benchmark target.

Reproduce the comparison locally.

Use:

the same machine
the same compiler profile
the same input
the same semantics
the same thread count
the same benchmark harness methodology.

Only local reproducible measurements count.

==================================================
NEW COMPETITOR QUALIFICATION
==================================================

Before adding a newly discovered competitor as a hard gate:

validate:

1. Is it actually maintained/usable?
2. Does it implement the same algorithm/capability?
3. Can outputs be made semantically equivalent?
4. Is its API doing the same amount of work?
5. Does it rely on precomputation?
6. Does Verbora's benchmark include equivalent precomputation?
7. Does it use SIMD?
8. Does it require CPU features?
9. Does it use unsafe?
10. Does it trade huge memory usage for speed?
11. Does it have narrower Unicode semantics?
12. Is it genuinely faster locally?

Then classify:

EXACT
NARROWED_EXACT
PARTIAL
TECHNIQUE
UNFAIR.

Only EXACT/NARROWED_EXACT become mandatory GREEN gates.

==================================================
COMPETITOR DISCOVERY AGENT
==================================================

Add a dedicated independent agent:

COMPETITOR SCOUT

Its responsibility is NOT to optimize Verbora.

Its responsibility is to try to defeat Verbora.

For every capability it must search for:

- faster crates;
- newer crates;
- specialized implementations;
- SIMD implementations;
- algorithm-specific crates;
- indexing/search libraries that solve the same primitive faster.

Its goal is:

find the strongest possible opponent.

If it finds a competitor faster than the current leader:

that competitor becomes the new target.

This agent should run independently from the Verbora optimization agents to
avoid confirmation bias.

==================================================
CHALLENGER REVALIDATION
==================================================

When a capability finally becomes GREEN:

run the COMPETITOR SCOUT one final time for that capability.

Ask:

"Is there another credible Rust implementation we have not benchmarked that
could invalidate this performance leadership claim?"

Only after this final challenger search can the capability be considered
competitively closed.

==================================================
PERMANENT COMPETITIVE POLICY
==================================================

Add this principle to AGENTS.md:

> Verbora's performance target is not a fixed list of libraries.

> The target is the fastest fair semantically-equivalent Rust implementation
> currently known.

> Before making or renewing a performance-leadership claim, search for new
> specialized competitors.

> If a newly discovered implementation is faster, Verbora's corresponding
> benchmark returns to RED and the optimization loop reopens.

> Performance leadership is continuously challengeable.

==================================================
EXPANDED DEFINITION OF DONE
==================================================

In addition to the existing Definition of Done:

[✓] triple_accel evaluated for exact Distance scenarios
[✓] fast_radix_trie evaluated for exact Trie scenarios
[✓] fst evaluated for frozen Trie/dictionary/fuzzy scenarios
[✓] fast_symspell evaluated for exact Spellcheck scenarios
[✓] snowball_stemmers_rs evaluated per exact stemmer language

[✓] specialized competitor discovery completed for EVERY capability
[✓] newly discovered credible competitors locally benchmarked
[✓] fastest exact competitor identified for every capability
[✓] Verbora beats that fastest competitor
[✓] final competitor-scout pass completed after optimization

The final requirement remains:

EVERY EXACT / NARROWED_EXACT HARD GATE == GREEN.

Not parity.

Not close.

Not "competitive".

GREEN.