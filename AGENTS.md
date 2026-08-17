# AGENTS.md

## Project Mission

Verbora is a high-performance, Rust-native natural language processing
toolkit — tokenization, stemming, phonetic matching, string distance,
n-grams, normalization, inflection, a trie, transliteration, WordNet,
TF-IDF, sentiment analysis, and classifiers.

Verbora holds itself to a **correctness-first** standard: every behaviour the
toolkit exposes is specified deliberately, documented, and pinned by tests that
assert the specified behaviour directly. Correctness is defined by Verbora's own
specification and test suite — nothing else.

The scope Verbora owns is:

* functionality;
* algorithms;
* public APIs;
* supported languages;
* tokenizers;
* stemmers;
* classifiers;
* phonetics;
* string distances;
* TF-IDF;
* WordNet;
* sentiment;
* inflections;
* n-grams;
* normalization;
* datasets;
* models;
* serialization;
* persistence;
* edge cases.

The question every change must answer is:

> **What behaviour does Verbora define, and what is the most efficient,
> idiomatic Rust implementation of that behaviour?**

Verbora is an independent, Rust-native toolkit, designed and built from
scratch. It is not a port, a rewrite, or a compatibility layer for any other
library.
---

# Core Principles

Every change to this repository must respect these principles, in this order:

```text
1. Correctness
2. Specified, test-pinned behaviour
3. Rust-native architecture
4. Performance
5. Memory efficiency
6. API quality
7. Maintainability
8. Portability
```

Performance is NOT an afterthought.

Rust-native architecture and performance must influence design decisions from the beginning.

However:

> An optimization that changes specified behaviour is not an optimization.

---

# Source of Truth

The primary source of truth is this repository:

```text
the specification in each crate's rustdoc
the tests that assert it
```

A behaviour is only real when it is written down and asserted. Do NOT rely on:

* assumptions about how a similar library behaves;
* prior knowledge of other toolkits;
* undocumented incidental behaviour of the current implementation.

When determining behavior, inspect:

```text
the crate's module and item documentation
its unit and integration tests
its datasets and models
its examples
```

When documentation and implementation disagree, that is a bug: decide which one
is right, fix the other, and add a test that locks the decision in.

---

# Specified Behaviour

The target is:

```text
every public behaviour documented and test-pinned
```

Not:

```text
mostly documented
tests to be written later
obvious enough from the code
```

If a public API can do something, that something is documented and has a test.

This includes obscure functionality and edge cases.

---

# What "Specified" Means

Specification is more than a function signature.

For each feature document and assert:

```text
inputs
outputs
defaults
configuration
errors
edge cases
Unicode behavior
language-specific behavior
empty input
ordering
floating-point behavior
serialization
persistence
determinism
observable semantics
```

APIs are designed for Rust callers first; ergonomics and semantics are chosen,
not inherited.

---

# Rust-Native Design

Design for Rust from the start. Never carry over a shape from a
dynamically-typed language because it is the familiar one.

Do NOT reproduce:

```text
prototype-style object models
callback-oriented APIs
dynamic typing
excessive runtime dispatch
mutable object graphs
temporary object creation
```

Design around Rust.

Prefer where appropriate:

```text
borrowing
lifetimes
&str
&[T]
iterators
IntoIterator
traits
enums
generics
Result
Option
Cow
RAII
static dispatch
strong typing
zero-cost abstractions
Send + Sync
immutable shared state
```

The implementation must feel like a Rust library, because it is one.

---

# Performance Is a First-Class Requirement

This project has a strong performance objective.

For every significant implementation ask:

```text
Can we avoid an allocation?

Can we avoid a copy?

Can we avoid a clone?

Can we borrow instead of own?

Can this operation be lazy?

Can we stream instead of materialize?

Can memory be reused?

Can we improve cache locality?

Can we reduce pointer chasing?

Can we precompute invariant data?

Can we use a more efficient algorithm?

Can repeated work be eliminated?

Can batch processing improve throughput?

Can independent work be parallelized?

Can SIMD help?

Can memory mapping help?

Can we reduce I/O?

Can initialization be lazy?
```

Do not assume code is fast because it is written in Rust.

Performance must be measured.

---

# Performance Philosophy

The preferred optimization cycle is:

```text
correctness
    ↓
specification tests
    ↓
benchmark
    ↓
profile
    ↓
identify bottleneck
    ↓
optimize
    ↓
unit tests
    ↓
golden-file tests
    ↓
benchmark again
```

Do NOT optimize based only on intuition when profiling or benchmarking can answer the question.

---

# API Design Philosophy

Where useful, APIs should support different performance levels without sacrificing ergonomics.

Conceptually:

```text
Simple API
+
Lazy API
+
Zero-copy API
+
Reusable-buffer API
+
Batch API
+
Parallel batch API
```

Not every subsystem needs every variant.

Add variants only when they make semantic and performance sense.

---

# Convenience APIs

The common use case must remain simple.

For example:

```rust
let tokens = tokenizer.tokenize(text);
```

should be possible.

Users should NOT need to understand scratch buffers, lifetimes, Rayon, or internal memory management to perform basic NLP operations.

Performance-oriented APIs are additional capabilities, not replacements for ergonomic APIs.

---

# Efficient Primitives First

Whenever possible, convenience APIs should be implemented on top of efficient primitives.

For example, conceptually:

```rust
tokenizer.tokens(text)
```

may provide a lazy primitive.

Then:

```rust
tokenizer.tokenize(text)
```

can collect that iterator.

And:

```rust
tokenizer.tokenize_into(text, &mut output)
```

can extend a reusable buffer.

Do NOT implement multiple independent versions of the same algorithm unless technically necessary.

Prefer:

```text
one optimized core
      ↓
multiple ergonomic interfaces
```

This minimizes:

* bugs;
* semantic divergence;
* maintenance cost.

---

# Iterator-First Design

Whenever an operation naturally produces a sequence, evaluate an iterator-based API.

Candidates include:

```text
tokenization
n-grams
WordNet relationships
document traversal
feature extraction
normalization pipelines
stemming pipelines
```

Example:

```rust
for token in tokenizer.tokens(text) {
    process(token);
}
```

Prefer lazy pipelines when they avoid unnecessary intermediate allocations.

For example:

```rust
tokenizer
    .tokens(text)
    .map(|token| stemmer.stem(token))
    .filter(...)
    .for_each(process);
```

is conceptually preferable to:

```text
tokenize
→ Vec
→ stem
→ Vec
→ filter
→ Vec
→ process
```

when equivalent semantics can be maintained.

---

# IntoIterator

Evaluate `IntoIterator` when a type naturally represents a consumable sequence.

Do not implement it merely for stylistic reasons.

It should improve:

* composability;
* ergonomics;
* performance;

without introducing hidden allocations.

---

# Zero-Copy

Zero-copy should be actively pursued where semantically possible.

Prefer:

```text
&str instead of String
&[T] instead of Vec<T>
borrowed slices instead of copied substrings
Cow when output may be borrowed or owned
```

For example, a tokenizer operating on:

```text
"The quick brown fox"
```

should ideally be capable of returning references into the original input when no transformation is required.

Do NOT allocate a new `String` for every token unless necessary.

---

# Borrowed vs Owned

When an operation sometimes returns unchanged data and sometimes needs transformed data, evaluate:

```rust
Cow<'a, str>
```

or another appropriate abstraction.

Do not use `Cow` automatically.

Use it when it reduces allocations while keeping the API understandable.

---

# Reusable Buffers

For high-throughput workloads, evaluate APIs that allow memory reuse.

Example:

```rust
let mut buffer = Vec::new();

for text in documents {
    buffer.clear();
    tokenizer.tokenize_into(text, &mut buffer);
    process(&buffer);
}
```

This may avoid:

```text
allocate
process
free
allocate
process
free
...
```

and replace it with:

```text
allocate
reuse
reuse
reuse
...
```

`*_into` APIs should be introduced where benchmarks or algorithm characteristics justify them.

Do NOT create `*_into` variants indiscriminately.

---

# Scratch Space

Algorithms requiring temporary memory should be evaluated for reusable scratch-space APIs.

Example:

```rust
let mut scratch = DistanceScratch::new();

for (a, b) in pairs {
    distance_with_scratch(a, b, &mut scratch);
}
```

Potential candidates include:

```text
Levenshtein
Damerau-Levenshtein
parsers
feature extraction
stemming
phonetics
```

Maintain a convenient API for normal use.

---

# Batch APIs

For operations commonly performed repeatedly, evaluate dedicated batch APIs.

Candidates include:

```text
tokenization
stemming
phonetics
normalization
sentiment
classification
string distances
TF-IDF
```

A batch API should provide actual opportunities for optimization such as:

```text
buffer reuse
preallocation
shared initialization
cache locality
reduced synchronization
parallelism
```

Do not add a batch API that provides no meaningful advantage over:

```rust
items.iter().map(...).collect()
```

unless it materially improves ergonomics.

---

# Parallelism

CPU-bound independent operations should be evaluated for parallel processing.

Potential examples:

```text
documents
stemming batches
tokenization batches
classification
sentiment
distance calculations
corpus processing
```

Evaluate Rayon or equivalent strategies when appropriate.

Parallelism should normally be optional when it introduces significant dependencies.

For example:

```text
feature = "parallel"
```

may be appropriate.

Do NOT assume parallel is always faster.

Measure the crossover point.

Small workloads should generally avoid thread scheduling overhead.

---

# Streaming

Avoid requiring entire datasets to exist in memory when an algorithm can operate incrementally.

Evaluate streaming interfaces using:

```text
Iterator
IntoIterator
Read
BufRead
```

where appropriate.

Potential candidates:

```text
corpus processing
tokenization
TF-IDF ingestion
classifier training
WordNet parsing
large documents
```

---

# Allocation Discipline

Allocations in hot paths must be treated as a design concern.

Review frequent usage of:

```rust
String::from(...)
.to_string()
.clone()
format!(...)
.collect::<Vec<_>>()
.collect::<String>()
Box::new(...)
```

This does NOT mean these operations are forbidden.

It means they must not be used thoughtlessly in performance-sensitive code.

---

# Clone Discipline

Do NOT use `.clone()` merely to make the borrow checker happy.

Before cloning significant data, consider:

```text
borrowing
ownership redesign
Arc
references
lifetimes
moving ownership
different data layout
```

Cloning small trivial values is fine.

Cloning large strings, collections, models, or indexes in hot paths requires justification.

---

# Preallocation

When output size can reasonably be estimated, evaluate:

```rust
Vec::with_capacity(...)
String::with_capacity(...)
HashMap::with_capacity(...)
```

Avoid repeated growth and reallocations.

Do not grossly overallocate without evidence.

---

# Small Inputs

NLP frequently operates on small tokens.

Evaluate stack-oriented representations where appropriate:

```text
arrays
SmallVec
ArrayVec
```

before heap allocation.

Use benchmarks to determine whether the additional complexity is justified.

---

# String Processing

String processing is expected to be one of the primary hot paths.

Pay special attention to:

```text
UTF-8 traversal
character classification
normalization
token boundaries
temporary Strings
temporary Vec<char>
regex
substring creation
case conversion
```

Do not convert strings into `Vec<char>` by default.

Operate directly over bytes/chars/graphemes according to the actual semantic requirements.

---

# Unicode

Correct Unicode behavior is mandatory.

Explicitly distinguish:

```text
bytes
Unicode scalar values
grapheme clusters
tokens
```

Do not assume one representation fits every algorithm.

Performance optimizations must not silently break Unicode behavior.

---

# ASCII Fast Paths

For operations where ASCII dominates common workloads, evaluate a fast path:

```rust
if input.is_ascii() {
    fast_ascii_path(input)
} else {
    unicode_path(input)
}
```

Potential candidates:

```text
tokenization
stemming
phonetics
normalization
distance
```

Only retain specialized paths when benchmarks demonstrate meaningful improvement.

---

# Regex

Regex is a convenience, not a default. Before reaching for one, evaluate whether Rust can perform the same operation more efficiently using:

```text
byte scanning
char scanning
state machines
lookup tables
precompiled regex
```

Whatever the mechanism, the documented observable behavior must not change.

---

# Algorithms

The specification pins observable behavior, not the internal algorithm.

Any implementation is fair game as long as the documented results are exactly reproduced. Prefer the one with the best CPU and memory complexity.

Example:

A distance implementation that only needs the final result may use:

```text
O(min(n,m)) memory
```

instead of:

```text
O(n*m) memory
```

when results remain identical.

Prefer algorithmic improvements over micro-optimizations.

---

# Data-Oriented Design

For large structures evaluate:

```text
contiguous memory
compact representations
cache locality
hot/cold data separation
reduced pointer chasing
```

Do not build large pointer-chasing object graphs when Rust can represent the same information more efficiently.

---

# Collections

Do not choose `HashMap` automatically.

For important hot-path structures evaluate:

```text
Vec
sorted Vec
binary search
HashMap
BTreeMap
IndexMap
perfect hash
array lookup
specialized hashers
```

Consider:

```text
cardinality
mutation frequency
lookup frequency
memory
determinism
security
cache locality
```

Benchmark important decisions.

---

# Hashing and Security

Fast non-cryptographic hashers may be appropriate for trusted internal data.

Do NOT weaken hash-DoS resistance where untrusted external input can control large numbers of keys unless the threat model has been explicitly evaluated.

Performance must not silently compromise security.

---

# Static and Precomputed Data

Invariant linguistic data should be evaluated for compile-time or build-time generation.

Possible strategies:

```text
static arrays
sorted tables
perfect hash
lookup tables
generated Rust data
compact binary resources
```

Avoid rebuilding invariant structures on every initialization.

---

# Lazy Initialization

Large resources that are not always needed should be evaluated for lazy initialization.

Potential tools include:

```rust
OnceLock
LazyLock
```

Avoid forcing users of a small subsystem to pay initialization costs for unrelated functionality.

---

# WordNet Performance

WordNet requires dedicated performance engineering.

Evaluate:

```text
runtime text parsing
buffered indexed access
memory mapping
prebuilt indexes
compact binary representation
lazy loading
shared immutable indexes
zero/minimal-copy lookup
```

A possible architecture is:

```text
WordNet source files
       ↓
index builder
       ↓
optimized Rust representation
       ↓
mmap/read-only loading
       ↓
low-latency queries
```

The optimized representation must preserve the documented semantics and comply with source-data licensing.

Measure:

```text
startup time
lookup latency
throughput
memory footprint
allocations per lookup
concurrent query behavior
```

---

# Distance Algorithms

Performance-review all distance/similarity algorithms.

Potential optimizations include:

```text
reusable rows
scratch buffers
prefix trimming
suffix trimming
early exits
stack allocation
SmallVec
ASCII fast paths
SIMD
better algorithms
```

Always preserve results.

---

# TF-IDF

TF-IDF should be designed around naturally sparse data.

Evaluate:

```text
sparse representations
efficient term indexing
document-frequency storage
batch insertion
streaming ingestion
repeated-query optimization
parallel corpus processing
```

Avoid dense representations when unnecessary.

---

# Classifiers

Evaluate:

```text
feature representation
sparse features
model layout
probability calculations
allocation behavior
prediction hot paths
training hot paths
batch training
batch prediction
parallel prediction
```

Persistence and output behavior must remain compatible with the documented semantics.

---

# NLP Pipelines

Subsystems should compose efficiently.

Where practical, allow pipelines like:

```text
input
 ↓
tokenize
 ↓
normalize
 ↓
stem
 ↓
feature extraction
 ↓
consumer
```

without mandatory intermediate collections.

Iterator adapters may be appropriate.

Examples conceptually include:

```text
Tokens<I>
Normalized<I>
Stemmed<I>
NGrams<I>
```

Do not over-engineer the type system.

Performance and usability both matter.

---

# SIMD

SIMD may be used where profiling identifies suitable hot paths.

Potential candidates:

```text
distance algorithms
byte scanning
ASCII normalization
character classification
vector comparisons
batch operations
```

Preferred order:

```text
efficient scalar safe Rust
↓
compiler auto-vectorization
↓
portable SIMD where stable/appropriate
↓
architecture-specific optimization
```

Do not introduce SIMD merely because it sounds faster.

Benchmark it.

---

# Unsafe

`unsafe` is NOT prohibited, but it is a last-stage optimization tool.

Preferred order:

```text
better algorithm
↓
better ownership
↓
better layout
↓
fewer allocations
↓
better locality
↓
compiler-friendly safe Rust
↓
SIMD
↓
unsafe if still justified
```

Any `unsafe` code must:

```text
be isolated
be documented
state its invariants
have tests
have benchmarks proving its value
be reviewed carefully
use Miri where applicable
```

Never introduce `unsafe` for negligible performance gains.

---

# Public API Stability vs Performance

Before the API reaches stable public releases, do NOT preserve a poor API merely because it already exists in the current implementation.

The provisional API may be redesigned whenever a substantially better Rust-native API exists.

Once public stability guarantees exist, follow Semantic Versioning.

---

# Testing Requirements

Every feature requires tests.

Tests must cover:

```text
normal behavior
edge cases
invalid input
Unicode
language-specific behavior
empty input
serialization when applicable
persistence when applicable
```

Coverage is judged against the documented specification, not against the code:
if the rustdoc claims a behaviour, some test must fail when that behaviour breaks.

---

# Golden-File Testing

Where a subsystem's output is large or structured — tagger output, WordNet
lookups, classifier scores — pin it with committed golden files rather than
hand-written assertions.

The workflow is:

```text
generate output from a fixed, committed input corpus
↓
review it by hand, once
↓
commit it as the expected result
↓
diff every subsequent run against it
```

A golden file is only ever regenerated as a deliberate, reviewed change:
a diff in a golden file is a behaviour change and must be justified in the
commit message.

```text
input corpus
 ├── committed golden output ──► A
 └── current implementation  ──► B

assert A == B
```

Use exact equality whenever semantics permit.

Use floating-point tolerances only when mathematically justified.

---

# Regression Testing

Every reproducible bug or behaviour mismatch should produce a regression test.

Workflow:

```text
detect mismatch
↓
minimize reproduction
↓
write failing test
↓
fix implementation
↓
verify test
↓
keep test permanently
```

---

# Property-Based Testing

Use property-based testing where appropriate.

Potential tools:

```text
proptest
quickcheck
```

Useful domains include:

```text
distance algorithms
tokenization
normalization
stemming
n-grams
phonetics
```

Properties must be mathematically valid for the specific algorithm.

---

# Fuzzing

Use fuzzing where valuable, particularly for:

```text
string processing
Unicode
parsers
WordNet
serialization
complex tokenizers
```

Potential tool:

```text
cargo-fuzz
```

Panics reachable through safe public APIs should generally be treated as bugs.

---

# Benchmarking

Every performance-critical subsystem requires representative benchmarks.

Benchmark:

```text
latency
throughput
allocations where measurable
memory where measurable
scaling behavior
```

Use multiple input sizes.

Conceptually:

```text
tiny
small
medium
large
very large
```

---

# Cross-Implementation Benchmark Fairness

When benchmarking Verbora against any other library, both implementations must
receive equivalent:

```text
inputs
operations
workloads
preparation
```

Do not game benchmarks by excluding real costs from only one implementation.

---

# Benchmark API Variants

When multiple APIs exist, benchmark them independently.

For tokenization, for example:

```text
tokenize()
lazy iterator
tokenize_into()
batch
parallel batch
```

This allows users and maintainers to understand the actual tradeoffs.

---

# Performance Documentation

Maintain:

```text
BENCHMARKS.md
```

and/or:

```text
PERFORMANCE.md
```

with relevant information:

```text
operation
dataset
input size
baseline
simple API
optimized API
speedup
memory observations
hardware
compiler
build profile
```

Do not publish unexplained performance claims.

---

# Performance Matrix

Maintain:

```text
PERFORMANCE_MATRIX.md
```

for major subsystems.

Suggested dimensions:

```text
Lazy API
Zero-copy
Reusable memory
Batch
Parallel
Allocation reviewed
Profiled
Benchmarked
Specified
```

`N/A` is valid when justified.

Not every algorithm benefits from every optimization technique.

---

# Feature Matrix

Maintain:

```text
docs/FEATURE_MATRIX.md
```

for the state of every public feature.

A feature should progress through states similar to:

```text
NOT_STARTED
IN_PROGRESS
IMPLEMENTED
TESTED
SPECIFIED
```

Only:

```text
SPECIFIED
```

— documented, test-pinned, and benchmarked — means complete.

---

# Architecture Documentation

Important architectural decisions belong in:

```text
ARCHITECTURE.md
```

Use ADRs for significant decisions:

```text
docs/adr/
```

Examples:

```text
iterator architecture
WordNet storage
parallelism strategy
public API conventions
serialization formats
MSRV decisions
unsafe usage
```

---

# Workspace Philosophy

The project should remain modular.

The expected architecture is a Cargo Workspace containing focused crates with an umbrella/facade crate.

Conceptually:

```text
verbora
├── verbora-core
├── verbora-tokenizers
├── verbora-stemmers
├── verbora-distance
├── verbora-phonetics
├── verbora-tfidf
├── verbora-classifiers
├── verbora-wordnet
├── verbora-sentiment
├── verbora-inflectors
├── verbora-ngrams
└── ...
```

Actual crate boundaries should follow real domain and dependency boundaries.

Do not create crates merely for symmetry.

---

# Umbrella Crate

Users should be able to consume the project conveniently through an umbrella crate.

Conceptually:

```toml
[dependencies]
verbora = "..."
```

Advanced users should be able to depend on narrower crates when they want:

```text
smaller dependency graph
faster compilation
smaller binaries
specific functionality
```

---

# Cargo Features

Use Cargo features for meaningful optional capabilities.

Potential examples:

```text
wordnet
parallel
serde
```

`parallel` is real as of the Fase 2 performance pass: thirteen crates
(`verbora-spellcheck`, `verbora-wordnet`, `verbora-tagger`,
`verbora-distance`, `verbora-tokenizers`, `verbora-normalizers`,
`verbora-analyzers`, `verbora-transliterators`, `verbora-sentiment`,
`verbora-stemmers`, `verbora-phonetics`, `verbora-classifiers`,
`verbora-tfidf`) expose `par_*_batch` APIs behind it, following
`verbora-core`'s existing `serde` feature convention
(`parallel = ["dep:rayon"]`, never in `default`). See `# Rayon Policy` above
for the full permanent policy this established.

Avoid an unnecessarily complicated feature graph.

Default usage should remain ergonomic.

---

# Concurrency

Read-only resources should be shareable efficiently where appropriate.

Aim for:

```rust
Send + Sync
```

when semantically correct.

Avoid:

```text
global mutable state
unnecessary Mutex
unnecessary RwLock
coarse-grained locking
```

Prefer immutable shared structures.

---

# Threading Policy

Do not silently spawn threads for trivial operations.

Library users should retain reasonable control over execution.

Batch/parallel APIs should make expensive parallel behavior predictable.

---

# Rayon Policy

Rayon accelerates Verbora primitives; Rayon must never become the
implementation of Verbora primitives. A `par_*_batch` function's body should
be, in essence, `items.par_iter().map(<the existing sequential
function/method>).collect()`. If parallelizing an operation would require
writing new algorithmic logic that does not exist in the sequential path,
that is a sign the design is wrong, not that the logic should be duplicated.

Concretely, permanently:

- Single-item primitives must remain fully functional without the `parallel`
  feature. The workspace must compile, and every test must pass, both with
  `--no-default-features` and with `--all-features`. CI enforces both (the
  `features` job in `.github/workflows/ci.yml`).
- `rayon` is `optional = true` in every crate that uses it, gated behind a
  `parallel` feature that is never in `default`. Follow the exact convention
  `verbora-core`'s `serde` feature established: `parallel = ["dep:rayon"]`.
- Prefer explicit parallel batch APIs (`par_tokenize_batch`) over silently
  parallelizing an existing API when a feature is enabled. A caller should
  never have a function start consuming multiple cores because a Cargo
  feature happened to be on elsewhere in the dependency graph.
- Prefer coarse-grained data parallelism (parallelize across independent
  documents/items) over parallelizing tiny inner operations. Measure the
  real per-item cost before choosing granularity — several crates in this
  workspace (`verbora-stemmers`, `verbora-phonetics`) have per-item costs
  low enough that naive per-item `par_iter` would lose to Rayon's own
  dispatch overhead; both use per-document batching or explicit `par_chunks`
  instead, verified by benchmark, not assumed.
- Prefer native Rayon parallel iterators (`par_iter`, `into_par_iter`) over
  `par_bridge()` whenever the data already lives in a slice or owned
  collection.
- Prefer worker-local computation followed by reduction/merge over shared
  mutable global state (`Mutex<HashMap<...>>`) for parallel aggregation.
  Where a primitive's own state truly cannot be split and merged safely
  (see `verbora-tfidf::par_add_documents_batch`'s doc comment for a worked
  example of investigating this and choosing a narrower, provably correct
  design over a fragile one), it is acceptable and expected to parallelize
  only the pure, stateless portion of the work and keep the stateful part
  sequential, rather than inventing an unverified merge algorithm.
- Every `par_*` API requires sequential-vs-parallel benchmark evidence and a
  equivalence test asserting the two produce identical output over the same
  inputs (empty, one item, many items, Unicode, and whatever pathological
  cases the crate's own sequential suite already exercises).
- Avoid nested parallelism and oversubscription. Verbora must not configure
  Rayon's global thread pool implicitly — callers keep control of their own
  execution environment.
- A type is not made `Send`/`Sync` with `unsafe impl` to satisfy Rayon. If a
  type genuinely cannot be shared safely across threads (interior mutability
  that is load-bearing, not incidental — see `MaxEntClassifier`'s
  `Rc<RefCell<_>>` state or `PorterStemmerNl`'s sticky `Cell<bool>` flag),
  it stays sequential-only, documented as to why, rather than forced.

---

# Data Structures

Do not default blindly to `HashMap`. Every structure holding more than a
handful of entries on a real hot path should be chosen deliberately,
considering cardinality, mutation pattern, read/write ratio, cache locality,
memory footprint, and a benchmark where the choice isn't obvious.

- `rustc_hash::FxHashMap`/`FxHashSet` (already a workspace dependency) is the
  default replacement for `std::collections::HashMap`/`HashSet` on any
  string- or small-key hot path in this workspace — SipHash's DoS resistance
  is not a relevant property for keys drawn from the caller's own text, not
  from untrusted network input. Several crates (`verbora-ngrams`,
  `verbora-classifiers`, `verbora-util`) already do this; match the pattern
  rather than reaching for `std::collections::HashMap` by default in new
  code.
- Do not use concurrent collections (`DashMap`, `Mutex<HashMap>`,
  `RwLock<HashMap>`) for read-only or build-once/read-many workloads. Prefer
  `build → freeze → Arc<ImmutableIndex>` (see below). Reach for a concurrent
  collection only when there is genuine, ongoing concurrent *mutation* with
  measured contention — not merely because the surrounding code happens to
  run on multiple threads.
- Small, static, read-heavy datasets (character classes, phonetic/stemming
  rule tables) are frequently better served by a `match`, a sorted static
  array with binary search, or a compile-time/build-time-generated table
  than by a `HashMap` built at runtime. `verbora-wordnet`'s pointer-symbol
  table and `verbora-trie`'s flat arena are the reference examples already
  in this codebase — read their own doc comments before assuming a `HashMap`
  is the default answer for a new lookup table.

---

# Build → Freeze → Query

For any structure with a `build once, query many times` access pattern,
prefer mutable, convenient structures during construction and a compact,
immutable representation during query. `verbora-tagger`'s `build.rs` (parses
the reference JSON lexicons once, packs them into a binary format with
sorted-string arenas and offset tables, embeds the result via
`include_bytes!`) and `verbora-wordnet`'s `PrebuiltIndex` sidecar are the
worked examples already in this codebase — read either before designing a
new build-once/read-many dataset.

Read-heavy runtime paths should avoid locks whenever possible. Prefer
`Arc<ImmutableIndex>` for state shared read-only across threads over a
`Mutex`/`RwLock`-guarded mutable structure once construction has finished.

---

# Verbora-Native Extensions

Verbora hosts purpose-built indexes and data structures that exist because a
real problem needs them, not because some catalogue lists them.
`verbora-phonetics`'s `PhoneticIndex` (Fase 4, "Phonetic Neighbors") was the
first, and it establishes the permanent policy for all of them: an extension
ships as long as all of the following hold.

- **Self-identified.** An extension states plainly, in its own module
  documentation, what problem it solves and what it deliberately does not do —
  see `crates/verbora-phonetics/src/index.rs`'s opening lines. Its site
  documentation says the same — see `site/features/phonetic-index.md` for the
  pattern.
- **Strictly scoped.** An extension solves the one problem it names and stops
  there. `PhoneticIndex::neighbors` is phonetic candidate generation — a
  blocking step — not a search engine: it does not rank, does not apply an
  edit-distance threshold, and does not accept a query language; ranking is
  left to composition with `verbora-distance` at the call site. That
  boundary, stated in the crate's own doc comment, is the concrete precedent
  for scoping any future extension — name the one question the
  structure answers, and do not grow it into adjacent functionality (ranking,
  a query language, persistence) without the same evidence discipline this
  section requires for the extension itself.
- **Benchmark-justified like everything else.** Every representation choice
  inside an extension still goes through `# Data Structures`'s
  discipline above, with no exemption for being new. `PhoneticIndex` chose a
  compressed-sparse-row layout (`InlineCode` codes plus an offset table) over
  a frozen `HashMap` and a dense perfect-hash-indexed array only after
  benchmarking all three (`benches/phonetic_index.rs`), and the result was not
  a clean sweep for the chosen design: it is clearly the most memory-compact
  of the four (29.00 bytes/entry vs. 31–40 for the alternatives at 100K
  entries) but measurably slower on raw query latency than a hash-based
  alternative at the same scale — a trade-off documented on its own site page
  rather than smoothed over. Publishing the unfavourable number alongside the
  favourable one is the point, not an embarrassment to edit out: an
  extension's performance claims are held to exactly the same
  `# Performance Evidence Requirement` as every other crate, including the
  numbers that don't flatter the choice made.
- **Same architectural rules, no separate track.** `PhoneticIndexBuilder` /
  `PhoneticIndex` is `# Build → Freeze → Query` applied to a new domain, not
  an exception to it — mutable accumulation during build, one freeze into a
  compact `Send + Sync` structure, lock-free `Arc`-shared queries after. An
  extension gets no separate performance bar, no separate testing bar, and no
  separate documentation bar: `# Definition of Done — Feature` applies in
  full.

---

# Language Detection and Phonetic Strategy

`verbora-language` (Fase 5, "language-aware phonetics") is the workspace's
**second** purpose-built extension under the policy above — `PhoneticIndex`
(Fase 4) was the first. The crate says as much itself, in the first lines of
`crates/verbora-language/src/lib.rs`'s doc comment, and names the single
question it exists to answer.

The crate exists to answer a question `PhoneticIndex` cannot: *given a word
or document, which of Verbora's four phonetic encoders (`SoundEx`,
`Metaphone`, `DoubleMetaphone`, `SoundExDM`) should even be used?* It keeps
that question split into three deliberately separate layers — script
detection (`script.rs`), statistical language detection (`detect.rs`,
`whatlang_detector.rs`), and phonetic-strategy lookup (`strategy.rs`) —
composed only at the edge by `AutoPhoneticStrategy<D>` (`auto.rs`). Fase 5's
own spec (`Fase 5 Language.md`, section 38) requires seven permanent-policy
lines be added verbatim; each is below with the real code that enforces it,
in this crate's own house style of evidence over assertion.

> Language detection and phonetic encoding are separate concerns.

Enforced structurally, not just by convention: `LanguageDetector`/
`LanguageDetection` (`detect.rs`) know nothing about phonetics, and
`recommend(language: Language)` (`strategy.rs`) takes a `Language` value, not
a `&str` or a detector — there is no function anywhere in the crate that
takes raw text and returns an encoded phonetic key in one call. See
`lib.rs`'s "Three layers, kept separate on purpose" doc-comment section,
which names this explicitly as the reason the crate has three modules
instead of one `auto_phonetic_encode(input)` entry point.

> If the caller already knows the language, prefer explicit language
> selection over automatic detection.

`recommend(Language::German)` needs no detector, no feature flag, and no
model — it is a pure `match` over 22 arms. The benchmark suite's manual-path
group (E) measured it at **5.67–7.48 ns**; the auto-detect path
(`AutoPhoneticStrategy::detect_and_recommend`, group D) measured
28.09–147.98 µs depending on input length in the same report — the manual
path is **~4,260x to ~22,400x cheaper**. A caller who already knows the
language and calls `recommend` directly pays essentially nothing; only the
caller who reaches for `AutoPhoneticStrategy` pays for detection.

> Single-word language detection must never be presented as certainty when
> the evidence is ambiguous.

`WhatlangDetector::detect` halves — never hides — a candidate's confidence
when `whatlang`'s own `is_reliable()` says no (`whatlang_detector.rs`), so a
weak single-word signal is still visible but scores low enough to be
rejected by a sane threshold. `LanguageDetection::best_above` requires the
*caller* to supply that threshold — there is no built-in default anywhere in
the crate (`detect.rs`'s and `auto.rs`'s own doc comments say so explicitly).
`crates/verbora-language/tests/ambiguity.rs` (8 tests) asserts specific
short/ambiguous inputs — `"hotel"`, `"radio"`, `"piano"`, `"normal"`,
`"color"`, short proper names — must not resolve to a single confident
language.

> Name origin must not be treated as equivalent to language.

Stated in `lib.rs`'s own "Names are not language" doc-comment section:
`Language::Italian` means "this text's linguistic signal matches Italian,"
never "this name sounds Italian" — a surname can have Italian origins and
appear in an English sentence with no contradiction. Nothing in the crate
infers anything about a person from their name.

> Language-aware phonetic selection should return uncertainty/recommendations
> instead of inventing confidence.

`PhoneticStrategy::primary` is `Option<PhoneticRecommendation>`, and
`AutoResult::strategy` is `Option<PhoneticStrategy>` — both explicitly
`None` rather than a fabricated default. `strategy.rs`'s `recommend()`
returns `primary: None, alternatives: Vec::new()` for `Persian`, `Hindi`,
and `Chinese` specifically because no Verbora transliterator or encoder fits
Arabic, Devanagari, or Han script — the doc comment on that match arm calls
recommending one anyway "exactly the false confidence this module exists to
avoid."

> Automatic language detection should remain optional if it introduces
> significant dependencies, model size or startup cost.

`language-detection` is an `optional`, non-`default` Cargo feature
(`crates/verbora-language/Cargo.toml`) gating the crate's only dependency on
`whatlang` and its only real detector, `WhatlangDetector`. `Language`,
`Script`/`detect_script`, `recommend`/`recommend_for_script`, the
`LanguageDetector` trait itself, and `AutoPhoneticStrategy<D>` (generic over
any detector) all compile and pass their own tests with **zero** extra
dependencies — `cargo test -p verbora-language` (33 tests, no features) vs.
`cargo test -p verbora-language --all-features` (40 unit + 8 ambiguity
tests) both pass clean, per the benchmark agent's report. `whatlang` was
chosen only after a real comparison, documented in full below.

> Phonetic strategy selection must remain independent from generic
> search/ranking functionality.

`recommend()`/`PhoneticStrategy` never rank candidates, never apply an
edit-distance threshold, and never accept a query — the same scoping
discipline `# Verbora-Native Extensions` above requires of `PhoneticIndex`.
Composing a recommendation with actual candidate ranking is left to the call
site (e.g. pairing with `verbora-phonetics::PhoneticIndex` or
`verbora-distance`), exactly as `PhoneticIndex::neighbors` leaves ranking to
composition rather than growing into a search engine itself.

## Choosing `whatlang`: the real comparison

`crates/verbora-language/src/whatlang_detector.rs`'s own doc comment records
the evaluation against the other two actively-maintained Rust
language-detection crates, `lingua` and `whichlang`, before `whatlang` was
chosen:

| | `whatlang` | `lingua` | `whichlang` |
|---|---|---|---|
| License | MIT | Apache-2.0 | MIT |
| Dependencies | 1 (`hashbrown`) | ~15, incl. `rayon`, `dashmap`, per-language model crates | 0 |
| Coverage of this crate's 22 languages | 20/22 (missing Galician, Basque) | 21/22 (missing Galician) | 13/22 |
| Honest low-confidence signal | `is_reliable()` | self-reported accuracy tables only | none |
| Footprint | ~685 KB compiled-in frequency tables | up to ~300 MB of per-language FST models if all languages enabled | ~775 KB, baked-in weights |

`whichlang` is leaner still (0 dependencies) but covers only 13 of this
crate's 22 languages and has shipped two releases ever. `lingua`'s
dependency graph (`rayon`, `dashmap`, per-language model crates) is
disproportionate to "guess the language of a short phrase" and directly
conflicts with this workspace's dependency-light stance (`# Dependencies`).
`whatlang` was the one candidate simultaneously MIT-licensed, nearly
dependency-free, actively maintained, covering the language list, and
already exposing a reliability signal instead of forcing this crate to
invent one — the losing comparisons are recorded in the crate's own doc
comment alongside the winning one, per `# Performance Evidence Requirement`.

## Lazy initialization: evaluated, not added

`# Lazy Initialization` and Fase 5 section 31 both call for `OnceLock`/
`LazyLock` to be evaluated if language detection needs large models loaded
at runtime. A sibling benchmark agent measured this with an external
counting-`GlobalAlloc` probe (run as a separate, non-workspace Cargo
project — this workspace's `unsafe_code = "deny"` forbids writing that probe
inside the crate itself, the same constraint `verbora-phonetics/benches/
phonetic_index.rs` already documents hitting) and found:

- `whatlang 0.18.0`'s frequency tables (`src/trigrams/profiles.rs`, five
  `pub static ...: LangProfileList` arrays covering Latin, Cyrillic, Arabic,
  Devanagari, and Hebrew scripts) are compile-time constants baked into the
  binary's rodata — no runtime load, no file I/O, no deserialization.
- `WhatlangDetector::new()` measured **0 heap allocations, 0 bytes**,
  deterministic across repeated runs — there is nothing to construct.
- The one thing `whatlang` *does* build at runtime — `ALPHABET_LANG_MAP`, an
  inverted char→language map — is already behind a `std::sync::LazyLock`
  one layer down, inside `whatlang` itself (`src/alphabets/latin.rs`),
  process-wide.

`verbora-language` therefore does **not** wrap `WhatlangDetector` in its own
`OnceLock`/`LazyLock`: doing so would add an atomic check to every
`detect()` call to guard a cache holding nothing, which is exactly the
synchronization-for-its-own-sake `# Concurrency` above argues against. This
reasoning, and the numbers it rests on, are recorded in
`crates/verbora-language/benches/language.rs`'s own module doc comment —
follow that same "measure, then decide, then record the reasoning even when
the answer is 'no'" discipline for any future detector this crate might grow
that *does* need a real runtime-loaded model.

## Rayon: `par_detect_batch`, and explicitly not single-text detection

`crates/verbora-language/src/parallel.rs` adds exactly one function,
`par_detect_batch`, behind the `parallel` feature (`optional = true` in
`Cargo.toml`, not in `default`) — independent of `language-detection`,
since it is generic over any `D: LanguageDetector + Sync`, not tied to
`WhatlangDetector`. Its body is the canonical shape `# Rayon Policy` above
requires: `texts.par_iter().map(|text| detector.detect(text)).collect()`,
nothing more, with an equivalence test (`matches_the_sequential_loop_in_order`)
asserting it against the plain sequential `.map()` over the same detector.
`WhatlangDetector` needs no `unsafe impl Send`/`Sync` — it is `Copy` and
zero-sized, so both are automatic, matching `# Rayon Policy`'s rule against
forcing thread-safety with `unsafe impl`.

Per Fase 5 section 33 ("No utilizar Rayon para detectar el idioma de una
palabra"), single-text detection stays sequential: `LanguageDetector::detect`
and `WhatlangDetector::detect` never spawn a thread or touch Rayon
internally: only the explicit, opt-in `par_detect_batch` does, and only when
a caller has an actual batch. The benchmark agent's `par_batch` group
(`language-detection,parallel` features, `SHORT_TEXT`-sized items, 32 cores)
measured:

| batch | sequential | parallel | speedup |
|---|---|---|---|
| 16 | 461.4–482.7 µs | 84.98–91.44 µs | ~5.3x |
| 64 | 1.862–2.057 ms | 223.2–334.5 µs | ~6–7x |
| 256 | 7.270–7.294 ms | 735.7–960.9 µs | ~8–9x |
| 1024 | 29.15–29.37 ms | 2.643–3.510 ms | ~9–10x |
| 4096 | 120.7–123.3 ms | 8.255–8.893 ms | ~13–14x |

Parallel won at every tested size here, including the smallest (16) — no
sequential-favoring crossover was observed in this range, because a single
`detect()` call already costs tens of µs (see the `language_detection`
bench group), so Rayon's fork-join overhead is negligible by comparison.
This is the opposite finding from `verbora-stemmers`/`verbora-phonetics`
(`# Rayon Policy` above), whose sub-microsecond per-item costs make naive
`par_iter` lose at small batch sizes — the two results do not contradict
each other; they are exactly `# Rayon Policy`'s "measure the real per-item
cost before choosing granularity" applied honestly to two different cost
shapes, not a rule of thumb that generalizes without benchmarking each new
primitive.

---

# Beider-Morse Phonetic Matching

`verbora-phonetics`'s `beider_morse` module is the workspace's **third**
purpose-built extension under `# Verbora-Native Extensions` above —
`PhoneticIndex` (Fase 4) was the first, `verbora-language` (Fase 5) the
second. Unlike those two, it wasn't scoped by a numbered Fase spec: it was
proposed and approved mid-session, in direct response to a real gap this
crate's other four phonetic encoders (`SoundEx`, `Metaphone`,
`DoubleMetaphone`, `SoundExDM`) share — all four are tuned for one
language's (mostly English's) orthography, and none solve the problem a
genealogical name index actually has: the *same* family name plausibly has
several "correct" spellings depending on which country transcribed it. It
is held to exactly the same four-bullet policy as every other entry in
this section, not a lighter bar for being newer.

- **Self-identified.** `crates/verbora-phonetics/src/beider_morse/mod.rs`'s
  own doc comment states plainly what this is (a Verbora-native extension
  with no the reference counterpart), what problem it solves, and why its
  output type (`BeiderMorseCode`, a variable-length candidate list) is
  deliberately not `PhoneticCodes` — the same shape every other encoder in
  the crate returns.
- **Strictly scoped.** `BeiderMorse::encode`/`encode_language` generate
  candidate spellings; they do not rank them, do not apply an edit-distance
  threshold, and do not index anything. Composing with `PhoneticIndex` or
  `verbora-distance` is left to the call site — the same boundary
  `PhoneticIndex::neighbors` itself draws.
- **Benchmark-justified like everything else.** See
  `docs/PERFORMANCE_MATRIX.md`'s "Beider-Morse Phonetic Matching" section
  for the full aspect-by-aspect table and real Criterion numbers, published
  including the one genuinely surprising result (Ashkenazi's narrower
  10-language pool measuring *slower* than Generic's 18 on the same surname
  list, not faster — traced to guess confidence and candidate-set size, not
  raw language count, rather than smoothed over).
- **Same architectural rules, no separate track.** Rule tables compile
  lazily and are cached process-wide (`NameTypeData::table`, an
  `RwLock<HashMap<...>>` behind a `OnceLock`-initialized per-`NameType`
  singleton) — the same lazy-and-cached shape `# Lazy Initialization`
  requires elsewhere, not a bespoke pattern for this one module.

## Licensing: read before touching `data/beider-morse/`

The 127 embedded rule files are Apache-2.0-licensed data from Apache
Commons Codec — itself a Java re-implementation of Alexander Beider and
Stephen P. Morse's original, GPL-3.0-licensed PHP reference. Verbora copies
from the Apache-2.0 chain only (via a verbatim copy of the same corpus
`rphonetic`, a mature independent Rust port, ships in its own test assets),
never touches the GPL-3.0 PHP source, and preserves every file's own
embedded license header plus a top-level `data/beider-morse/NOTICE.md`
recording the full provenance chain. See `# Licensing` below for the
workspace's general policy this follows.

## No reference oracle — how correctness was actually verified

Every other Verbora-native extension in this section still has *something*
to lean on (`PhoneticIndex` reuses this crate's own already-parity-verified
encoders; `verbora-language` benchmarks against real competitor crates).
Beider-Morse has neither a reference implementation nor an existing
Verbora algorithm to check against, so correctness was established during
development against a disposable, non-dependency build of `rphonetic`
reading the *identical* rule-file corpus this crate embeds — chosen
specifically because it isolates engine-algorithm correctness (shared rule
data, independently-written engines) from rule-corpus correctness (not
independently re-verified). That process caught two real bugs before
landing:

- A rule alternative tagged with more than one language, `+`-joined (e.g.
  `gv[portuguese+spanish]`), was being looked up as one literal (and
  therefore unmatchable) language name and silently dropped — affecting
  roughly 94 rules across the corpus. Fixed by splitting on `+` and unioning
  each name's resolved `LanguageSet`, the same shape the language-guesser's
  own compound-tag parsing already used correctly.
- The Rules pass was passing an unmatched character through literally
  instead of silently skipping it, diverging from the reference
  implementation's own two genuinely different behaviors per pass (Rules:
  skip; Approx/Exact final pass: pass through) — confirmed by reading both
  call sites in the reference source, not assumed uniform. Only observable
  for characters no rule covers at all, such as the literal space `concat`
  mode (the real, if misleadingly-documented, default — see
  `BeiderMorse::new`'s own doc comment) fuses between words.

Post-fix, a 106-Generic-surname sweep matched the oracle exactly on 96.2%
(102/106); Ashkenazi (10 names), Sephardic (10 names), `RuleType::Exact` (12
names), and 16 explicit single-language calls across most of Generic's
language list all matched 100%. The remaining four Generic mismatches, plus
one of five prefix/multi-word cases, cluster around one still-open
word-final-consonant edge case (names ending `-poulos`/`-gh`) traced by hand
through the actual rule files without finding an attributable bug on
Verbora's side — recorded here rather than silently accepted, per this
workspace's own "measure and disclose, don't smooth over" discipline.

## Independent audit: one blocker found and fixed

Once the engine reached the correctness numbers above, a dedicated agent
audited the whole module fresh — the same practice this workspace's other
major features require at completion, not skipped here for being newer.
It found one real blocker and several minor issues; all were fixed
(the minor ones were false or stale doc claims and thin test coverage, not
behavioral bugs) except the still-open edge case already disclosed above.

**Blocker, fixed:** a repeated Generic name prefix (`"de de de ... cruz"`)
recursed through `combine_prefix_split` -> `self.encode` once per
repetition, and unlike ordinary multi-word splitting, the recombined string
at each level was roughly the *same* length as the level above it, not
shrinking — so recursion depth and per-level cost both scaled with input
length at once, compounding `PhonemeBuilder::apply`'s own (inherent to
every reference implementation's candidate-building shape) per-call cost of
rebuilding each candidate's whole accumulated text. Measured before the
fix: ~600 characters of a repeated prefix already cost ~150ms, ~3,000
characters cost 14+ seconds — from the fully public `encode`/
`encode_language`, no length guard, no doc warning. `encode_top`
(`mod.rs`) now caps prefix-splitting at 128 characters and the whole
normalized input at 512, both far beyond any real name and both verified to
collapse the measured 14-second case to single-digit milliseconds (locked
in by the `repeated_name_prefix_does_not_blow_up` regression test) without
changing output for any name in the verification sweep above.

Also fixed from the same audit: `BeiderMorseCode` gained an explicit
`compound: bool` field (the prefix/multi-word "one composed string, not
independent candidates" case was previously only distinguishable by reading
`encode`'s own doc comment — a real footgun for a caller who skips it); the
`\"` escape in rule-file quadruplets is now actually unescaped at compile
time (`rule.rs`'s `unescape_quote`) rather than surviving as a literal
2-character `\"` that could never match (dormant in the current corpus,
since its only 4 occurrences are all Rules-pass files where
`OnUnmatched::Skip` happened to mask the effect); a
`corpus_language_tags_all_resolve` test now guards every `[language]` tag
across the whole embedded corpus against silently collapsing a rule's
candidate branch to nothing; `engine.rs` and `lang.rs` (previously zero
direct test coverage) each gained targeted unit tests; and a handful of
false doc claims (a "debug-only warning path" that didn't exist,
`spellings`' ordering claimed as engine-order when it's actually always
alphabetical via the `BTreeMap`-based dedup) were corrected to match actual
behavior.

---

# Fuzzy Indexing (FuzzyIndex)

`verbora-spellcheck`'s `FuzzyIndex` is the workspace's **fourth**
purpose-built extension under `# Verbora-Native Extensions` above —
`PhoneticIndex` (Fase 4), `verbora-language` (Fase 5), and Beider-Morse
(above) came first. Like Beider-Morse, it wasn't scoped by a numbered Fase
spec: it was proposed and approved mid-session, after an explicit scope
discussion about the line between "candidate-generation primitive" (in
scope) and "search engine" (never in scope — see `# Verbora-Native
Extensions`'s own "does not rank, does not apply an edit-distance
threshold, does not accept a query language" boundary, which this feature
was deliberately designed to stay inside of rather than push against).

- **Self-identified.** `crates/verbora-spellcheck/src/fuzzy_index.rs`'s own
  doc comment states plainly what this is (a Verbora-native extension, no
  reference counterpart), what it answers (*which stored words are within
  edit distance `k` of this query?*), and why it's a BK-tree rather than
  the SymSpell-style deletion index the published literature usually
  recommends for this problem — a BK-tree's correctness follows directly
  from the triangle inequality, provable without a separate "over-generate
  then verify" step a deletion index needs, and `max_distance` is a
  query-time parameter rather than one fixed at build time.
- **Strictly scoped.** `FuzzyIndex::neighbors` generates candidates; it does
  not rank them, does not accept a caller-supplied cost `Options` (a
  deliberate restriction — an arbitrary cost assignment is not guaranteed
  to satisfy the triangle inequality the tree's pruning depends on for
  correctness, so accepting one could silently return incomplete results),
  and does not index anything but a flat word list. Ranking composes at the
  call site with `verbora_distance`, the same pattern
  `site/recipes/fuzzy-matching.md` already documented for `PhoneticIndex`
  and now documents for this as an alternative Step 1 (bucket by edit
  distance instead of by phonetic key — the two catch different typo
  shapes and compose rather than compete).
- **Benchmark-justified like everything else.** `FuzzyIndex` was built and
  measured against the honest baseline (a brute-force linear scan computing
  real Levenshtein distance, the same primitive `Spellcheck::get_corrections`
  already had no need to pre-index) before being kept — see
  `docs/PERFORMANCE_MATRIX.md`'s own entry for the real numbers: 2.4× faster
  at 100 words, widening to 3.6× at 20,000, i.e. the speedup *grows* with
  scale rather than converging toward parity, which is the shape that
  actually justifies pre-indexing over scanning. A SymSpell-style
  alternative was considered and explicitly not built this pass, for lack
  of its own benchmark evidence — recorded as a real gap, not silently
  assumed unnecessary.
- **Same architectural rules, no separate track.** Build → Freeze → Query,
  the same shape `PhoneticIndex`/`PrebuiltIndex` already establish:
  `FuzzyIndexBuilder` accumulates inserts, `.build()` freezes into an
  immutable `FuzzyIndex` with no interior mutability. Correctness is
  verified directly against the brute-force baseline over 3,000 real words
  and 248 full result-set comparisons (`tests/fuzzy_index.rs`), not spot-
  checked with a handful of hand-picked examples — the same "verify against
  ground truth" discipline this session applied to Beider-Morse via a live
  oracle, adapted here to a case where the ground truth is directly
  computable rather than needing an external reference implementation.

---

# Path-Compressed Trie Queries (FrozenTrie)

`verbora-trie`'s `FrozenTrie` (built by `Trie::freeze`) is the workspace's
**fifth** purpose-built extension under `# Verbora-Native Extensions`
above — `PhoneticIndex` (Fase 4), `verbora-language` (Fase 5), Beider-Morse,
and `FuzzyIndex` came first. Unlike those four, it targets a *measured
competitive loss*, not a new capability: the competitive audit's
`docs/PERFORMANCE_GAPS.md` entry 32 found `fast_radix_trie`, a
path-compressed radix map, beating `Trie::keys_with_prefix`/
`predictive_search` by 1.64×–2.19×, while `Trie` kept winning `build` and
`contains` against the same competitor — a targeted, single-operation gap,
not a general trie-performance problem.

- **Self-identified.** `crates/verbora-trie/src/frozen.rs`'s own module doc
  comment states plainly that this is a Verbora-native extension with no
  reference `trie` counterpart, and `Trie::freeze`'s own doc comment
  states exactly which operations it does and does not extend, and why.
- **Strictly scoped.** `FrozenTrie` implements only `contains` and
  `keys_with_prefix`/`iter_keys_with_prefix`/`keys` — the operations the
  measured gap and the benchmarked `predictive_search`/`contains_hit`/
  `contains_miss` groups are about. `find_matches_on_path`/`find_prefix`/
  `find_prefix_lengths` have no frozen counterpart: neither this crate's own
  benchmarks nor the competitive audit found a loss there, so compression was
  not extended to those top-down single-path walks just because it could be.
  Call the equivalent method on the un-frozen `Trie` for those.
- **Benchmark-justified like everything else, including the unfavourable
  number.** `FrozenTrie` closes most of the `predictive_search` gap and
  **overtakes** `fast_radix_trie` on the realistic single-letter-prefix shape
  (1.06× faster) — but still trails on full-corpus enumeration (1.45×
  slower), and its `contains` genuinely **regresses** against the plain
  `Trie` arena (1.65×–1.71× slower), which is published here, not edited out,
  exactly as `# Verbora-Native Extensions` above already requires for
  `PhoneticIndex`'s own unfavourable query-latency number. See
  `docs/PERFORMANCE_GAPS.md` entry 32's "Update" section and
  `docs/COMPETITIVE_BENCHMARKS.md` §1.18 for the full numbers and the
  architectural reason the trade-off runs this direction: `contains` only
  ever crosses as many branch points as the arena would too, and pays extra
  indirection per hop for no reduction in hop count, while enumeration visits
  every edge in the whole structure, where fewer total node-visits is a real
  saving. The shipped recommendation is to keep both representations, chosen
  per call site, not to replace one with the other.
- **Same architectural rules, no separate track.** `Trie::freeze` is
  `# Build → Freeze → Query` applied to this crate specifically:
  `Trie::add_string`/`add_strings` is unchanged (mutable build stays exactly
  as fast as it already was), `freeze()` is the one-time compression step
  (~1 ms for a 20,000-word corpus, comparable to `build` itself, not paid per
  query), and `FrozenTrie` itself is immutable after construction. It uses
  **no `unsafe`** anywhere — compressed edges are ranges into a shared
  `Vec<u16>` buffer, not raw pointers — unlike `fast_radix_trie`'s own
  dynamically-sized-node design, so the `unsafe`-acceptance question
  `docs/COMPETITIVE_BENCHMARKS.md` §1.18 originally flagged as a real
  decision this extension would need to make turned out not to be necessary.
  Correctness was checked two independent ways before any benchmark number
  was trusted: an 80-round randomized fuzzer inside `frozen.rs`'s own test
  module, and a separate adversarial audit agent with no visibility into the
  implementation's design reasoning, which wrote and ran its own fresh
  adversarial tests and validated they had real teeth by deliberately
  introducing and then catching two bugs before reverting them — the same
  "independent audit of a major feature" pattern this session already
  applied to Beider-Morse.

---

# SymSpell-Style Deletion Index (DeletionIndex)

`verbora-spellcheck`'s `DeletionIndex`/`DeletionIndexBuilder` is the
workspace's **sixth** purpose-built extension under
`# Verbora-Native Extensions` above — `PhoneticIndex` (Fase 4),
`verbora-language` (Fase 5), Beider-Morse, `FuzzyIndex`, and `FrozenTrie`
came first. Like `FrozenTrie`, it targets a *measured competitive loss*:
`docs/PERFORMANCE_GAPS.md` entry 35 found `fast_symspell` (a real, pinned
third-party deletion-index crate) beating `FuzzyIndex`'s own query speed by
a margin that widens with corpus size (2.15×–66.7×) — real evidence,
already flagged in `FuzzyIndex`'s own doc comment as the documented
alternative not yet built for lack of that evidence at the time.

- **Self-identified.** `crates/verbora-spellcheck/src/deletion_index.rs`'s
  own module doc comment states plainly what this is (a Verbora-native
  extension, no reference counterpart), the algorithm (SymSpell-style
  deletion precomputation, over-generate-then-verify), and the trade-off
  against `FuzzyIndex` up front, before any implementation detail.
- **Strictly scoped.** `DeletionIndex::neighbors` generates candidates and
  verifies them against real Levenshtein distance; it does not rank, does
  not accept a caller-supplied cost `Options` (same reasoning as
  `FuzzyIndex`), and does not index anything but a flat word list.
  `max_distance` is fixed at construction and any query asking for more is
  silently capped, never silently over-promised — a real, disclosed
  structural ceiling, not a soft default a caller could reasonably miss.
- **Benchmark-justified like everything else, including the unfavourable
  number.** `DeletionIndex` is **13×–25× slower to build** than `FuzzyIndex`
  at every size measured, and **loses the query benchmark at the smallest
  size tested** (100 words, `FuzzyIndex` 1.73× faster) — both published, not
  edited out. It wins query speed from 1,000 words up, by a margin widening
  to 54.3× at 20,000 — the same shape `fast_symspell` itself showed against
  `FuzzyIndex`, which is the real evidence that justified building this at
  all. See `docs/PERFORMANCE_MATRIX.md`'s own entry for the full numbers.
- **Same architectural rules, no separate track.** Build → Freeze → Query:
  `DeletionIndexBuilder` accumulates inserts, `.build()` freezes into an
  immutable `DeletionIndex` with no interior mutability. A real correctness
  bug was found and fixed *during* implementation, before any benchmark
  number was trusted, not after: deletion generation initially operated on
  `char`s, which silently mismatches
  [`verbora_distance::levenshtein`]'s own UTF-16-code-unit granularity for
  astral (non-BMP) input — the exact class of bug this crate's own
  `edits.rs`/`units.rs` already documents for `Spellcheck`'s edit generator.
  Fixed to operate on `Vec<u16>` throughout, with a dedicated
  astral-character-heavy correctness test added specifically because the
  shared ASCII-only benchmark corpus can never exercise this risk class.
  Independently re-verified by a second, adversarial audit agent with no
  visibility into the implementation's own design reasoning — the same
  "independent audit of a major feature" pattern already applied to
  Beider-Morse and `FrozenTrie`.

---

# Archived Data and Memory Mapping

Large immutable linguistic datasets must be *evaluated* for memory-mapped
access (`memmap2`) and/or an archived zero-copy representation (`rkyv`) —
not necessarily adopted. `memmap2`/`rkyv` are not currently dependencies of
any crate in this workspace; adopting either is a real cost (the workspace's
`unsafe_code = "deny"` lint at `[workspace.lints.rust]` means `memmap2`'s
mapping call would be this workspace's first `unsafe` code) that must be
weighed against a measured alternative, not assumed to be worth it.

`verbora-wordnet` is the worked example: its own benchmark (`benches/
wordnet.rs`) measures four storage strategies (`Pread`, `LazyResident`,
`Resident`, `Indexed`) plus a `PrebuiltIndex` binary sidecar head-to-head,
and a memmap2 feasibility review (Fase 2 performance audit) concluded the
existing safe backends already deliver what mmap would provide — near-zero
cold start via `Pread` (66-87 µs to open-plus-one-lookup, vs. milliseconds
for a fully resident load) and cheap warm lookups via `Resident`/`Indexed` —
with no `unsafe` and no new dependency. Keeping the existing architecture
was the correct, evidence-based decision here; it will not automatically be
the correct decision for every future large dataset. Re-run the same
evaluation — real benchmarks on the real dataset, compared against what
mmap/rkyv would concretely buy — rather than either defaulting to the
existing pattern or defaulting to mmap/rkyv without measuring.

Where persistence exists, decide explicitly between Serde and `rkyv` per
dataset:

- Serde remains the default choice for portable, interoperable
  serialization — configuration, user-facing persistence (e.g.
  `verbora-classifiers`' `to_json`/`save`, matching the reference's own JSON
  format), anything that needs to round-trip through a human-readable or
  cross-language format.
- `rkyv` is for internal, read-heavy, immutable archived representations
  where reduced deserialization cost has been measured to matter. It must
  not be introduced as a global replacement for Serde.
- Do not archive an inefficient runtime structure blindly. Design the
  frozen representation for the query workload first (compact IDs, offset
  tables, interned strings), then choose the persistence mechanism.
- Archive validation, alignment, lifetime ownership, and format/schema
  versioning are part of the performance design for any mmap-backed
  archive, not optional follow-up work.
- Zero-copy claims require evidence that the complete query path avoids the
  claimed copies/allocations — "zero-copy archive access", "no per-query
  allocation" and "lazy decoding" are related but not interchangeable
  claims; say precisely which one applies and how it was verified.

---

# Performance Evidence Requirement

Performance claims require benchmark or profiling evidence, run and quoted,
not estimated. Do not describe an API as "faster", "zero-copy",
"allocation-free" or "high-performance" without a command that was actually
run and a number that came out of it. Multiple APIs for the same conceptual
operation (`tokenize()` / `tokens()` / `tokenize_into()` / `tokenize_batch()`
/ `par_tokenize_batch()`) must exist only when they solve measurably
different workloads, and each must document why it exists, when to use it,
and what it costs relative to its siblings.

---

# Competitive Benchmark Policy

Fase 6 added a second benchmarking axis alongside Verbora's own internal
baselines above: Verbora measured against real, version-pinned competitors
from the Rust ecosystem. `benchmarks/competitive/` (its own, isolated Cargo workspace —
never a member of this repository's root `[workspace]`, so a rival crate
never becomes a dependency of a published `verbora-*` crate),
`docs/COMPETITIVE_BENCHMARKS.md` (the research matrix: every competitor
considered, selected or rejected, and why) and
`docs/PERFORMANCE_GAPS.md` (every real loss this audit found, investigated,
never hidden) are its permanent record. These eleven rules are permanent
policy, not a one-phase checklist:

> Competitive performance claims require reproducible benchmark evidence.

Every number in `benchmarks/competitive/results/results.json` traces to a
committed raw Criterion file under `results/raw/` and a documented `cargo
bench` command — see [Competitive benchmarks § Reproducing these
numbers](https://addlayerio.github.io/verbora/benchmarks/competitive#reproducing-these-numbers).

> Competitor benchmarks must use semantically equivalent workloads.

`docs/COMPETITIVE_BENCHMARKS.md` marks every row `Yes`, `Partial` or `No
fair competitor` before a single benchmark is written, and a `Partial` row
is only benchmarked over a documented, narrowed input domain proven equal by
a correctness test first — e.g. rphonetic's Metaphone reconfigured to
`Some(32)` to match Verbora's real default, verified by
`tests/phonetics_correctness.rs` before any timing was trusted.

> Never cherry-pick benchmark results.

An independent fairness audit (see this file's own `# Independent Review`
section) cross-referenced every "Verbora loses" row in `results.json`
against `docs/PERFORMANCE_GAPS.md` before Fase 6's results were published,
specifically to catch an omitted loss.

> If Verbora loses a valid benchmark, publish the result and investigate the
> performance gap instead of removing the benchmark.

`docs/PERFORMANCE_GAPS.md`'s 37 entries are exactly this — Levenshtein vs.
`rapidfuzz`'s bit-parallel algorithm (~91× at 1024 chars), seven of nine
Snowball stemmers vs. both `rust-stemmers` and `snowball_stemmers_rs`,
Metaphone vs. `rphonetic`, `fast_symspell`'s 1,686×–2,769× query-speed win at
`max_distance=2` alongside Verbora's own 25×–34× construction-speed win
(a real trade-off, not a one-sided loss), `fast_radix_trie`'s
`predictive_search` win specifically (Verbora still wins `build`/`contains`
against it) — each with a source-cited likely reason and, where one exists, a
flagged-but-not-implemented optimization opportunity, per this file's own
"measure first, then a focused follow-up phase" discipline.

> Every module with a real ecosystem counterpart must carry one as a
> benchmarked baseline.

Of the 19 audited modules, the three genuine exceptions (language detection,
script detection, phonetic indexing) were confirmed by searching the
ecosystem for an equivalent, not assumed from the module name —
`docs/COMPETITIVE_BENCHMARKS.md` § 4 records the confirmation for each.

> Rust competitors must be selected based on relevance and adoption, not on
> convenience.

Every competitor in `docs/COMPETITIVE_BENCHMARKS.md`'s dossiers carries its
own download counts, GitHub stars, license and last-maintenance date, and a
stated reason it was selected over the alternatives found — e.g. `strsim`
(~990M downloads, the ecosystem's de-facto standard) over lower-adoption
Dice-coefficient alternatives that diverge on the same benchmarked variant.

> Parallel-vs-sequential comparisons must disclose thread counts.

Every table in [Competitive benchmarks](https://addlayerio.github.io/verbora/benchmarks/competitive)
states its thread count is 1 — no benchmark in `benchmarks/competitive/`
exercises a parallel API on either side, and Verbora's own sequential-vs-
parallel numbers, with thread counts disclosed, are kept on the
[Parallelism](https://addlayerio.github.io/verbora/performance/parallelism)
page instead, never mixed into a single-thread comparison.

> Benchmark documentation must be generated from structured raw results
> where practical.

`benchmarks/competitive/scripts/collect-results.py` reads Criterion's raw
`estimates.json` output into `results/results.json`; every table published
from it — in `docs/PERFORMANCE_GAPS.md` and on the competitive-benchmarks
site page — is computed from that file's `median_ns` values, not retyped by
hand.

> Performance claims in documentation must never be manually invented.

The `DO NOT USE FAKE NUMBERS` discipline this file's `# Performance Evidence
Requirement` section already states applies identically here: a benchmark that could not be run is reported as "not
measured" or excluded with a stated reason, never estimated or
extrapolated.

> Accuracy-sensitive NLP tasks must report quality metrics alongside speed
> when practical.

Language detection's speed table never appears without its accuracy table
alongside it (13 languages × 4 length tiers, sourced from the OHCHR UDHR
corpus: `benchmarks/competitive/datasets/README.md`) — `whichlang`'s
16×–411× speed advantage over Verbora's `WhatlangDetector` is reported next
to the accuracy numbers showing it also has the lowest raw accuracy at the
one tier that speed gap is largest on, not as an unqualified "faster" claim.
The Naive Bayes classifier accuracy report (`cargo test --test
classifiers_accuracy`) follows the same rule.

> Verbora's performance target is not a fixed list of libraries. The target
> is the fastest fair semantically-equivalent Rust implementation currently
> known. Before making or renewing a performance-leadership claim, search
> for new specialized competitors. If a newly discovered implementation is
> faster, Verbora's corresponding benchmark returns to RED and the
> optimization loop reopens. Performance leadership is continuously
> challengeable.

Added when a later competitor-discovery pass found `triple_accel`
(SIMD-accelerated Levenshtein/Hamming, already benchmarked in
`benches/distance.rs`) and pinned four more specialized crates this same
pass surfaced — `fast_radix_trie` (radix/patricia trie, `unsafe`-internal),
`fst` (BurntSushi's finite-state transducer, both as a frozen-dictionary
competitor and via its Levenshtein automaton as a fuzzy-lookup competitor),
`fast_symspell` (a second SymSpell-family implementation, `memmap2`+`rkyv`
+`triple_accel`-backed), and `snowball_stemmers_rs` (Wolf Garbe's
Snowball-generated per-language stemmers) — none of which appeared in the
original Fase 6 research pass, confirming the ecosystem this policy targets
does not hold still. `RED`/`GREEN` here means the same thing `#
Performance Evidence Requirement` means everywhere else in this file:
a claim traces to a real, reproducible local measurement or it is not made
— "GREEN" is never awarded for beating an old competitor list, only the
currently-known-fastest fair one, and losing to a newly-discovered
competitor (verified locally, not from that competitor's own marketing
copy — see this section's own "reproducible benchmark evidence" rule above)
reopens the investigation rather than being quietly left stale.

---

# Error Handling

Do not use panics for normal user-controlled error conditions.

Prefer:

```rust
Result<T, E>
Option<T>
```

with meaningful error types.

Panics should represent violated internal invariants, not ordinary invalid input.

---

# Dependencies

External crates may be used.

Do NOT reimplement algorithms solely for ideological purity.

Before adding a dependency evaluate:

```text
correctness
performance
maintenance status
license
API quality
binary impact
compile-time impact
transitive dependencies
semantic compatibility
```

If an existing crate provides the correct algorithm efficiently, reuse may be preferable.

If it differs from Verbora's specified behavior, adapt it at the boundary or implement the necessary behavior internally.

---

# Licensing

Verbora's own source code and the external linguistic resources it ships must
be treated separately.

Audit:

```text
source code
WordNet
datasets
dictionaries
corpora
models
fixtures
generated resources
third-party crates
```

Maintain where applicable:

```text
LICENSE
NOTICE
THIRD_PARTY_LICENSES.md
```

Never assume a dataset inherits the license of code that consumes it.

---

# Agent Parallelism

When performing large tasks, use multiple agents/subagents in parallel whenever work can be safely decomposed.

Do NOT serialize independent work unnecessarily.

Good decomposition boundaries include:

```text
tokenizers
stemmers
distance algorithms
phonetics
TF-IDF
classifiers
WordNet
sentiment
inflections
compatibility testing
performance analysis
licensing
CI
```

---

# Agent Ownership

Parallel agents should have explicit ownership of files/crates.

Avoid multiple agents modifying the same central files simultaneously.

Shared files such as:

```text
workspace Cargo.toml
umbrella crate
ARCHITECTURE.md
FEATURE_MATRIX.md
PERFORMANCE_MATRIX.md
```

require coordinated integration.

---

# Agent Definition of Done

An implementation task is NOT complete with code alone.

A responsible agent should deliver, as applicable:

```text
implementation
unit tests
ported/reference tests
differential tests
documentation
benchmarks
migration matrix update
performance matrix update
```

---

# Independent Review

For major features or optimizations, use an independent reviewer agent.

The reviewer should actively search for:

```text
missing specified functionality
missing exports
missing languages
missing tests
semantic differences
unnecessary allocations
unnecessary clones
temporary collections
missed iterator opportunities
poor cache locality
unnecessary locks
missed batch opportunities
missed parallelism
```

The author of a subsystem should not be the only authority deciding that it is complete.

---

# Performance-Specialist Agents

For large optimization work, parallelize performance review.

Possible responsibilities:

```text
Strings / Tokenization
Algorithms / Distances
TF-IDF / Sparse Structures
Classifiers
WordNet / I/O / mmap
Memory / Allocations
Iterator Architecture
Concurrency / Batch
SIMD / Low-Level Hot Paths
```

Every optimization must be benchmarked before and after.

---

# Quality Gates

The workspace should remain compatible with:

```bash
cargo fmt --all --check

cargo clippy \
    --workspace \
    --all-targets \
    --all-features \
    -- -D warnings

cargo test \
    --workspace \
    --all-features

cargo doc \
    --workspace \
    --all-features \
    --no-deps
```

Also use where appropriate:

```text
cargo-audit
cargo-deny
cargo-fuzz
Miri
Criterion
```

---

# Documentation Is Part of the Code

> Verbora documentation is a first-class part of the project.

> Any change affecting public behavior, APIs, performance characteristics,
> supported languages, usage patterns, Cargo features or recommended execution
> strategies MUST update the GitHub Pages documentation in the same change.

> Documentation drift is a bug.

The documentation site lives in:

```text
site/          VitePress source, theme, and the three validation scripts
```

It started as an mdBook build and was migrated to VitePress; see
[Why VitePress](site/reference/docs-are-code.md) for the reasoning,
including the reversed part.

It is published automatically to GitHub Pages from `main` by
`.github/workflows/docs.yml`. There is no manual deploy step, and the same job
that publishes also validates, so the published site cannot diverge from what
was checked.

A functionality that exists in code but is not correctly documented is:

```text
INCOMPLETE
```

## What must trigger a documentation change

```text
new feature
new API
new trait
new tokenizer
new stemmer
new algorithm
new language
new Cargo feature
changed defaults
changed errors
changed behavior
deprecated API
performance optimization
new iterator
new _into API
new batch API
new parallel API
changed allocation behavior
changed recommendation
```

The last one is the most often forgotten. If an optimization changes which API
should be recommended for a workload, the change is not finished until these are
updated:

```text
decision tree
comparison tables
performance guide
recipes
benchmarks
```

## The five gates

`.github/workflows/docs.yml` enforces:

```text
1. every published Rust snippet compiles AND its assertions pass
2. cargo test --workspace (unit + integration + doctests)
3. cargo doc with -D warnings and -D rustdoc::broken_intra_doc_links
4. npx vitepress build (a sidebar entry with no page behind it fails the build)
5. internal link and anchor check, including inside raw HTML, plus a check
   that every page is reachable from the sidebar
```

Run them locally before opening a PR:

```bash
cargo test --workspace
python3 site/check-snippets.py
(cd site && npm run build)
python3 site/check-links.py
```

Neither mdBook's `mdbook test` nor VitePress has a snippet test runner:
rustdoc's `-L`-only extern resolution can't populate the prelude for
edition-2018-or-later snippets, so it cannot compile anything that
`use verbora_*`, whichever site generator sits in front of it. `check-snippets.py`
extracts every published snippet into a generated example under
`crates/verbora-examples/examples/` and lets Cargo link it directly, which also
lets the assertions actually run.

## Honesty rules

These are what make the rest of the documentation worth reading:

```text
No invented APIs.
    Every function, type and method named in the docs must exist in the source.
    Where something does NOT exist — parallel APIs, scratch buffers, an
    unimplemented subsystem — say so plainly. Do not describe a plausible design
    as though it shipped.

No invented numbers.
    Only cite measurements that exist in docs/PERFORMANCE.md or
    benches/results/*.json. Everywhere else, describe asymptotics and allocation
    behavior read from the source, and label the section "not yet benchmarked".

Known-wrong behaviour is documented as such.
    Where a behaviour is knowingly kept because callers depend on it, name it,
    explain it, and link it. Do not quietly keep it.

Placeholder crates are documented as placeholders.
    A crate whose lib.rs says "Implementation in progress" has no public API.
    It belongs on the roadmap page, not on the features overview.
```

---

# Choosing the Right API

> Whenever multiple APIs solve the same conceptual problem, documentation MUST
> explain why each variant exists and when users should choose it.

A group such as:

```text
tokenize()
tokens()
tokenize_into()
tokenize_batch()
```

must never appear without, for each member:

```text
use-case
trade-off
performance characteristics
allocation behavior
recommendation
```

And the design corollary:

> If a real difference between two APIs cannot be explained, the second API
> should not exist.

Where a subsystem has more than one call shape, its documentation page must
carry a `Choosing the Right API` section containing:

```text
1. conceptual explanation
2. comparison table
3. examples
4. decision tree
5. benchmarks when they exist
6. concrete recommendations
```

Never present a performance-oriented API as universally superior. The simple API
is not the bad API — `tokenize()` is the correct choice for the large majority of
programs, and the documentation must say so.

---

# Performance-Oriented APIs

> Every performance-oriented API must document the performance problem it
> solves.

For example, `tokenize_into()` must explain:

```text
why it exists
when to use it
when not to use it
how memory reuse works
how it differs from tokenize()
how it differs from lazy iteration
what benchmark evidence supports it
```

An API whose only justification is "it is faster" is not documented. State the
workload it is faster *for*, and what it costs the caller — in Verbora's case
usually a mutable buffer to manage and a `clear()` that is a silent correctness
bug when forgotten.

Where the honest answer is that no measurement exists, say that, and describe
the allocation behavior instead of implying a speedup.

---

# Definition of Done — Feature

A feature is complete only when:

```text
[✓] Behavior specified in writing before implementation
[✓] Implementation complete
[✓] Rust-native API reviewed
[✓] Unit tests complete
[✓] Edge cases covered
[✓] Golden files reviewed and committed where applicable
[✓] Unicode reviewed where applicable
[✓] Performance implications reviewed
[✓] rustdoc complete on every public item
[✓] GitHub Pages documentation updated in the same change
[✓] Usage examples written, and compiling in CI
[✓] "Choosing the Right API" guidance written where variants exist
[✓] FEATURE_MATRIX updated
```

For performance-sensitive functionality also require:

```text
[✓] Allocation behavior reviewed
[✓] Lazy/iterator opportunity reviewed
[✓] Zero-copy opportunity reviewed
[✓] Buffer reuse reviewed
[✓] Batch opportunity reviewed
[✓] Parallel opportunity reviewed
[✓] Representative benchmark exists
[✓] The performance problem the API solves is documented
[✓] Comparison table and decision tree updated
```

Implementation plus tests is **not** a complete feature. The full bar is:

```text
implementation
+
tests
+
performance review
+
benchmarks where applicable
+
rustdoc
+
GitHub Pages
+
usage examples
+
Choosing the Right API guidance when applicable
```

---

# Definition of Done — Project

The project is complete only when:

```text
100% public functionality inventoried
100% target functionality implemented
100% supported languages implemented
100% specified behaviours test-pinned
100% FEATURE_MATRIX = SPECIFIED

Rust unit tests PASS
integration tests PASS
golden-file tests PASS
fmt PASS
clippy PASS
docs PASS

license audit complete
independent functional audit complete

performance audit complete
hot paths profiled
critical operations benchmarked
memory behavior reviewed
PERFORMANCE_MATRIX complete
```

---

# Anti-Patterns

Avoid introducing the following without strong justification:

```text
Vec<String> where borrowed &str is possible
collecting intermediate Vecs unnecessarily
clone() to bypass ownership design
repeated parsing of invariant data
repeated regex compilation
large pointer-heavy object graphs
global locks
hidden thread creation
unnecessary dynamic dispatch
unnecessary heap allocation
dense structures for naturally sparse data
runtime construction of compile-time-known data
dynamic-language architecture reproduced literally
unsafe before algorithmic optimization
SIMD without benchmarks
parallelism without benchmarks
```

---

# Decision Rule

When several implementations are functionally equivalent, prefer the one that best combines:

```text
Rust idioms
↓
better algorithmic complexity
↓
fewer allocations
↓
less copying
↓
lower memory usage
↓
better cache locality
↓
streaming/laziness
↓
safe concurrency
↓
higher measured throughput
```

Do not sacrifice readability for insignificant gains.

For substantial hot-path improvements, complexity may be justified when documented and benchmarked.

---

# The Tokenizer Example

Tokenization represents the general design philosophy of this project.

A user should have a simple API:

```rust
let tokens = tokenizer.tokenize(text);
```

A high-performance pipeline should ideally support lazy processing:

```rust
for token in tokenizer.tokens(text) {
    process(token);
}
```

Composition should avoid intermediate collections:

```rust
tokenizer
    .tokens(text)
    .map(|token| stemmer.stem(token))
    .for_each(process);
```

Repeated workloads may benefit from memory reuse:

```rust
let mut output = Vec::new();

for text in documents {
    output.clear();
    tokenizer.tokenize_into(text, &mut output);
    process(&output);
}
```

Large independent workloads may benefit from batch or parallel processing.

The important principle is NOT the specific method names.

The principle is:

> **Provide an ergonomic default while exposing Rust-native execution paths that allow advanced users to eliminate unnecessary work.**

Apply this reasoning throughout the entire library.

---

# Final Engineering Philosophy

Never ask:

> "How is this normally done in some other language?"

Ask:

> "What behavior do we want, and what is the best possible Rust implementation of it?"

The specification defines the functionality.

Rust defines the architecture.

Benchmarks define performance reality.

Tests define correctness.

The desired result is:

```text
100% of the specified functionality
+
100% Rust-native architecture
+
strong type safety
+
memory safety
+
zero/low-cost abstractions
+
lazy evaluation
+
zero-copy where possible
+
buffer reuse
+
efficient memory layouts
+
streaming
+
batch processing
+
safe parallelism
+
SIMD where justified
+
high throughput
+
low latency
+
low memory overhead
```

The end goal is:

> **An independent, Rust-native toolkit: specified deliberately, pinned by its
> own tests, architecturally native to Rust, and engineered from the ground up
> for performance.**
