# Verbora transliteration — standards research

Research phase for Verbora Phase 1. **No implementation was done**; this records
what the standards research established, so the implementation brief can be
written against verified facts rather than assumptions.

Full agent transcripts are in [`research-raw/`](research-raw/) (9.8 MB). They are
verbose but contain the primary-source quotes and the URLs behind every claim
below.

| Transcript | Topic | Completeness |
|---|---|---|
| `ad07c643b4858e264` | **Dataset licensing** | complete |
| `a6bfec81091f74aad` | **Cyrillic + Greek** | complete, primary-sourced |
| `a43026521ce7c85c0` | **Japanese** | complete, primary-sourced |
| `af74d56da25835eea` | **Indic / Devanagari** | complete |
| `a46b25661379c1ab9` | **Arabic** complete; Hebrew/Indic partial | partial — see below |
| `ad3141e6a736a28cc` | Korean | **failed** (session limit) |
| `a5712fd04f4f9d78f` | Architecture/design synthesis | **failed** (session limit) |

## Findings that change the design

### 1. Licensing decides the architecture, not performance

The single most actionable result. Verdicts:

| Dataset | Licence | MIT-crate verdict |
|---|---|---|
| **Unihan / UCD** | `Unicode-3.0` (OSI) | ✅ **backbone** — permissive, notice only |
| **CLDR transforms** | `Unicode-3.0` | ✅ **backbone** — and ships `-BGN` variants |
| CC-CEDICT | **CC BY-SA 4.0** | 🔴 not under bare MIT — separate crate |
| JMdict / JMnedict / KANJIDIC2 | **CC BY-SA 4.0** | ⚠️ separate crate; **strip SKIP codes**; §4 update duty |
| KANJIDIC SKIP codes | NC-SA **or** royalty terms — sources contradict | 🔴 **strip entirely** |
| IPADIC | `NAIST-2003` | ✅ ship full notice |
| NAIST-jdic | BSD-3 | ✅ cleanest |
| UniDic cwj/csj | GPL/LGPL/**BSD-3**, your election | ✅ elect BSD-3 explicitly |
| UniDic CHJ/dialect | CC BY-NC-SA | 🔴 do not vendor |
| pypinyin `pinyin-data` | **mislabelled MIT** — contains CC BY-SA + unlicensed scraped data | 🔴 do not vendor |
| `transliteration.eki.ee` | none — reproduces ISO tables | 🔴 never — unlicensed ISO laundering |
| ISO 9 / 843 / 233 / 259 / 15919 / 7098 | © ISO | ❌ reference by number only |
| BGN/PCGN | US §105 + **OGL v3** | ✅ best fallback (flag Greek/ELOT) |

**Consequence:** build on **Unihan + CLDR transforms**, both `Unicode-3.0`. CLDR
already ships BGN-variant romanization tables, which sidesteps the entire
ISO/UN/Crown-Copyright thicket for most scripts. Quarantine every ShareAlike
dataset in its own `CC-BY-SA-4.0` crate; Cargo aggregation is a collection, not
an adaptation, so the obligation stays contained.

Also: **Lindera is not a compliance model.** `lindera-cc-cedict` declares MIT
while fetching CC BY-SA data, and ships no NOTICE.

### 2. `ṛ` is an interop bomb

**IAST `ṛ` (U+1E5B) = ऋ, a syllabic vowel. ISO 15919 `ṛ` (U+1E5B) = ड़, a
retroflex consonant.** Same codepoint, disjoint meanings, undetectable from the
string. `kṛṣṇa` read under the wrong scheme becomes garbage.

**The scheme must be a required, non-defaulted parameter. Never auto-detect
between IAST and ISO 15919.**

### 3. Several "standards" are families, not functions

ISO 15919 requires you to *declare* your options (`vowel_option`,
`nasalization_option`, `urpha_option`) — its conformance clause says so
explicitly, and strict nasalization is **not reversible** while simplified is.
ISO 843 (Greek) likewise: *"An application can choose one, and only one, of these
types… The application must explicitly declare the type adopted."*

So the config surface is not optional polish; conformance depends on it.

### 4. Japan replaced its national standard on 2025-12-22

Cabinet Notification No. 4 abolished the 1954 Kunrei-shiki notification and made
a Hepburn-based table the standard — but **not** Modified Hepburn. It rejects
Hepburn's two famous irregularities:

| | Hepburn / ALA-LC | Japanese national standard (2025) |
|---|---|---|
| 抹茶 | `matcha` | **`maccha`** |
| 新橋 | `Shimbashi` | **`Shinbashi`** |

If we say "Hepburn", we must say *which*. Also: the new standard's
**doubled-vowel mode is round-trippable to kana; macron mode is not** (おう/おお
merge). Choose doubled mode where reversibility matters.

### 5. Kana→rōmaji is closed; Kanji→reading is not

- Kana→rōmaji: **107 table entries + 5 bounded contextual rules.** Closed problem.
- Kanji→reading: 2,136 jōyō kanji / 4,388 readings; ~62% have multiple readings;
  JMnedict has **~720,000 name entries and still does not close it.**
- **No published reading-accuracy figures exist** for MeCab, Sudachi, Juman++ or
  Kakasi. Reported as a positive finding, not a search gap. Do not promise
  accuracy we cannot measure.
- The 2025 standard's own answer for personal names is **ask the person**.

*(Folklore correction: 生 does **not** have "100+ readings" — Taishukan's own FAQ
says they have never seen such a list; the real count is ~13.)*

### 6. Arabic: normalization and transliteration are different operations

- **Normalization** is within-script and *deliberately* lossy (search recall).
- **Transliteration** is cross-script; losing information is a *defect*.

They must not share a code path. And on unvocalised text, only **ISO 233:1984**
is well-defined — every other standard requires vowels the input does not carry.
ISO 233-2 literally mandates injecting them, which is why it is not reversible.
Best neural diacritization is ~3% word error on clean newswire; a rule table gets
nowhere near.

**Do not implement sun-letter assimilation or taa-marbuta construct-state by
rule.** Both need morphosyntax — CLDR declined to do the former for exactly this
reason, with a comment saying you cannot mechanically tell the article ال from a
word-initial ال.

### 7. Unicode normalization is step zero, and it is script-specific

| Script | Behaviour | Consequence |
|---|---|---|
| **Arabic** | NFC **composes** hamza carriers; no Arabic composition exclusions | run NFC and carriers are canonical |
| **Hebrew** | all 34 precomposed forms are **composition exclusions** | NFC **decomposes**; points are always separate combining marks |
| **Devanagari** | U+0958–095F nukta letters are composition exclusions | `NFC(क़) = क + ़` — NFC destroys precomposed forms |
| **Greek** | oxia collapses onto tonos under NFC | a precomposed lookup table **must** NFC first or it silently misses all polytonic text |

NFKC also **inserts a space** decomposing `U+FE70`, so tokenisation must run
after normalization; and tatweel survives NFKC and must be stripped explicitly.

### 8. Cyrillic is not one problem

Serbian and Macedonian are **digraphia**, not romanization: Cyrillic→Latin is
lossless and mechanical, but **Latin→Cyrillic is not** (`nadživjeti` = `d`+`ž`,
not `dž`) and needs a ~500-entry exception lexicon.

Recommended defaults are the **national systems**, which for Ukrainian, Bulgarian
and Macedonian are now *identical* to current BGN/PCGN and are pure ASCII.
**Do not default to ISO 9** — `Ûrij Žukov` is unreadable, and it needs combining
marks (`g̀ l̂ n̂ d̂ J̌`) that break fonts, search and collation.

## What is still missing

- **Korean** — agent died before reporting. Hangul decomposition is pure
  arithmetic (no dataset, no licence), but Revised Romanization is
  pronunciation-based and needs phonological rules.
- **Hebrew standards tables** — ISO 259 / ALA-LC / Academy: unverified.
- **The architecture/API design synthesis** — never written.
- ISO 15919's diacritic for **ख़** — three reputable sources disagree.
- **ISO/DIS 15919 is at stage 40.99**: a 2nd edition lands imminently and
  retitles. Do not hard-freeze against the 2001 tables.

## Scope boundary confirmed

Verbora-native transliteration must live in a **separate crate** from
`verbora-transliterators`, which is a *parity* crate reproducing the reference's
`TransliterateJa` exactly and is not allowed to "improve" its output. The reference
parity percentages and `PARITY_VERIFIED` status must never be affected by
Verbora-native capabilities.
