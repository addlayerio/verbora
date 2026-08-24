//! Carry's suffix tables.
//!
//! Checked-in data. Each table is a suffix -> replacement map: steps 1 and 2
//! have two tables each (minimum radix 1, then 2) and step 3 has one. Entries
//! are sorted by key so a lookup is a binary search; only exact-key lookups are
//! ever performed on them, never iteration, so the sort order is not
//! observable.
//!
//! # Grounding, and what the walk found
//!
//! These tables have now been compared against the algorithm they implement —
//! Paternostre, Francq, Lamoral, Wartel and Saerens, *Carry, un algorithme de
//! désuffixation pour le français* (juillet 2002), whose appendix lists every
//! rule twice: an abbreviated first listing and a complete second one. The
//! two listings are not nested: the abbreviated one holds three the complete
//! one lacks, the complete one holds ten the abbreviated lacks, and their union
//! is **229 distinct step-1 suffixes**. This file holds 228 of them — `îtés`
//! (abbreviated listing only) is absent. It is also a byte-exact transcription
//! of the table this
//! crate was ported from, upstream `natural`'s
//! `lib/natural/stemmers/Carry/stepConfs.js` — all 246 entries, keys and
//! replacements alike. There is no transcription slip here of the kind
//! [`super::gates`] found in the German gate.
//!
//! What the walk did find is that two entries are *wrong where they fire*, and
//! both are pinned by name in [`crate::carry`]'s suite rather than left to be
//! rediscovered:
//!
//! * **`("ien", "i")` has no plural partner.** It is one of the five entries
//!   below that appear in no published listing of Carry; upstream added it, and
//!   it earns its place (`musicien` and `musique` both reach `music` only
//!   because of it). But `iens` is absent, so the singular loses `-ien` and the
//!   plural loses only `-s`: `milicien` stems to `milic` while `miliciens`
//!   stems to `milicien`. Over the 240 `-ien` singular/plural pairs of a
//!   346,205-word French dictionary, **230 stem apart**; [`crate::PorterStemmerFr`]
//!   splits none of them. Adding `("iens", "i")` unifies all 230 and splits
//!   nothing that is unified today — an owner's call, because it would take
//!   this table one entry further from both the publication and upstream.
//! * **`("yeux", "oeil")` is a whole-word rule in a suffix table.** `yeux` is
//!   the suppletive plural of `œil`, not a suffix, and [`crate::carry`]'s loop
//!   starts one character short of the whole word — so this entry can never
//!   fire on the only word it was written for (`stem("yeux")` is `"yeux"`) and
//!   fires on all seven French words that merely end in `-yeux`, making a
//!   non-word of each: `joyeux` -> `jooeil`, `ennuyeux` -> `ennuoeil`,
//!   `soyeux` -> `sooeil`. It also splits the adjective from its feminine,
//!   which stems sanely (`joyeuse` -> `joy`). Every reference implementation
//!   reproduces this, so removing it is likewise a deliberate divergence rather
//!   than a repair.
//!
//! # Divergences from the publication, itemized
//!
//! Three step-1 suffixes here are in neither appendix listing: `eur` and
//! `âtes`, which repair omissions the paper plainly has (it lists `eurs`
//! without `eur`, and drops `âtes`, *vous chantâtes*, outright), and `ien`,
//! above, which repairs nothing and has no plural partner. `euse` and `iere`
//! were once counted here too and should not have been — both are in the
//! abbreviated listing, inside its run of `-e` suffixes. Step 3 diverges in four places: it
//! adds `mm`, `pp` and `ss` to the paper's seven rules and omits the paper's
//! `gu -> g`. `tests` pins all nine so none can move quietly.
//!
//! Two further readings were taken from the paper where the paper contradicts
//! itself, and both follow its plural: `oise -> o` (the paper writes `oise` ->
//! ε but `oises` -> `o`) and `ouse -> ou` (the second listing writes `ous`, the
//! first `ou`).
//!
//! # What the walk ruled out
//!
//! `tests::every_table_entry_can_actually_fire` in [`crate::carry`] walks all
//! **246** entries through `transform` itself and requires each to fire, so no
//! entry is dead the way a Lancaster rule in the wrong section is. That is a
//! proof rather than a sample: no key here holds `j`, `k` or `w`, so a base
//! ending in one of those blocks every longer key, and the size test is always
//! satisfiable by a long enough base. Shadowing can therefore never be total
//! in this engine, and a future edit that breaks either half of that argument
//! fails loudly.

/// Step 1, minimum radix 1 (219 entries).
static STEP1_T0: &[(&str, &str)] = &[
    ("a", ""),
    ("able", ""),
    ("ables", ""),
    ("ade", ""),
    ("ades", ""),
    ("age", ""),
    ("ages", ""),
    ("ai", ""),
    ("aient", ""),
    ("aire", ""),
    ("aires", ""),
    ("ais", ""),
    ("aise", ""),
    ("aises", ""),
    ("ait", ""),
    ("alement", "al"),
    ("amment", ""),
    ("ance", ""),
    ("ances", ""),
    ("ant", ""),
    ("ante", ""),
    ("antes", ""),
    ("ants", ""),
    ("as", ""),
    ("asse", ""),
    ("assent", ""),
    ("asses", ""),
    ("assez", ""),
    ("assiez", ""),
    ("assions", ""),
    ("assons", ""),
    ("at", ""),
    ("ate", ""),
    ("ates", ""),
    ("ats", ""),
    ("au", ""),
    ("aux", "al"),
    ("cque", "c"),
    ("cques", "c"),
    ("e", ""),
    ("ea", ""),
    ("eai", ""),
    ("eaient", ""),
    ("eais", ""),
    ("eait", ""),
    ("eant", ""),
    ("eante", ""),
    ("eantes", ""),
    ("eants", ""),
    ("eas", ""),
    ("easse", ""),
    ("eassent", ""),
    ("easses", ""),
    ("eassiez", ""),
    ("eassions", ""),
    ("eau", ""),
    ("ee", ""),
    ("ees", ""),
    ("eille", "eil"),
    ("eilles", "eil"),
    ("elle", ""),
    ("ellement", "el"),
    ("elles", ""),
    ("ement", ""),
    ("ements", ""),
    ("emment", ""),
    ("ence", ""),
    ("ences", ""),
    ("ent", ""),
    ("entes", ""),
    ("ents", ""),
    ("eons", ""),
    ("eont", ""),
    ("er", ""),
    ("era", ""),
    ("erai", ""),
    ("eraient", ""),
    ("erais", ""),
    ("erait", ""),
    ("eras", ""),
    ("erent", ""),
    ("eresse", ""),
    ("eresses", ""),
    ("erez", ""),
    ("erie", ""),
    ("eries", ""),
    ("eriez", ""),
    ("erions", ""),
    ("erons", ""),
    ("eront", ""),
    ("es", ""),
    ("esse", ""),
    ("esses", ""),
    ("ete", ""),
    ("etes", ""),
    ("ette", ""),
    ("ettes", ""),
    ("etude", ""),
    ("etudes", ""),
    ("eur", ""),
    ("eure", ""),
    ("eures", ""),
    ("eurs", ""),
    ("euse", ""),
    ("euses", ""),
    ("eux", ""),
    ("ez", ""),
    ("eâmes", ""),
    ("eât", ""),
    ("eâtes", ""),
    ("f", "v"),
    ("fs", "v"),
    ("gue", "g"),
    ("gues", "g"),
    ("i", ""),
    ("ien", "i"),
    ("ient", ""),
    ("ients", ""),
    ("ier", ""),
    ("iere", ""),
    ("ieres", ""),
    ("iers", ""),
    ("iez", ""),
    ("ions", ""),
    ("ir", ""),
    ("ira", ""),
    ("irai", ""),
    ("iraient", ""),
    ("irais", ""),
    ("irait", ""),
    ("iras", ""),
    ("irent", ""),
    ("irez", ""),
    ("iriez", ""),
    ("irions", ""),
    ("irons", ""),
    ("iront", ""),
    ("is", ""),
    ("isme", ""),
    ("ismes", ""),
    ("issaient", ""),
    ("issais", ""),
    ("issait", ""),
    ("issant", ""),
    ("issante", ""),
    ("issantes", ""),
    ("issants", ""),
    ("isse", ""),
    ("issement", ""),
    ("issements", ""),
    ("issent", ""),
    ("isses", ""),
    ("issez", ""),
    ("issiez", ""),
    ("issions", ""),
    ("issons", ""),
    ("iste", ""),
    ("istes", ""),
    ("it", ""),
    ("ite", ""),
    ("ites", ""),
    ("ition", ""),
    ("itude", ""),
    ("itudes", ""),
    ("ité", ""),
    ("itée", ""),
    ("itées", ""),
    ("ités", ""),
    ("ière", ""),
    ("ières", ""),
    ("nne", "n"),
    ("nnes", "n"),
    ("oise", "o"),
    ("oises", "o"),
    ("ons", ""),
    ("ont", ""),
    ("ouse", "ou"),
    ("ouses", "ou"),
    ("que", "c"),
    ("ques", "c"),
    ("r", ""),
    ("rs", ""),
    ("s", ""),
    ("t", ""),
    ("tion", ""),
    ("tions", ""),
    ("trice", ""),
    ("trices", ""),
    ("ts", ""),
    ("ttes", ""),
    ("té", ""),
    ("tés", ""),
    ("uction", ""),
    ("ulle", "ul"),
    ("ulles", "ul"),
    ("usse", ""),
    ("ussent", ""),
    ("usses", ""),
    ("ussiez", ""),
    ("ussions", ""),
    ("x", ""),
    ("yeux", "oeil"),
    ("âmes", ""),
    ("ât", ""),
    ("âtes", ""),
    ("èrent", ""),
    ("ète", ""),
    ("ètes", ""),
    ("é", ""),
    ("ée", ""),
    ("ées", ""),
    ("és", ""),
    ("étude", ""),
    ("études", ""),
    ("îmes", ""),
    ("îrent", ""),
    ("ît", ""),
    ("îtes", ""),
    ("ûmes", ""),
];

/// Step 1, minimum radix 2 (12 entries).
static STEP1_T1: &[(&str, &str)] = &[
    ("ateur", ""),
    ("ateurs", ""),
    ("ation", ""),
    ("ations", ""),
    ("iation", ""),
    ("iations", ""),
    ("ication", ""),
    ("ications", ""),
    ("teur", ""),
    ("teurs", ""),
    ("ure", ""),
    ("ures", ""),
];

/// Step 2, minimum radix 1 (1 entries).
static STEP2_T0: &[(&str, &str)] = &[("i", "")];

/// Step 2, minimum radix 2 (5 entries).
static STEP2_T1: &[(&str, &str)] = &[
    ("ation", ""),
    ("el", ""),
    ("ent", ""),
    ("ition", ""),
    ("tion", ""),
];

/// Step 3, minimum radix 1 (9 entries).
static STEP3_T0: &[(&str, &str)] = &[
    ("ll", "l"),
    ("mm", "m"),
    ("nn", "n"),
    ("pp", "p"),
    ("qu", "c"),
    ("ss", "s"),
    ("t", ""),
    ("tt", "t"),
    ("y", ""),
];

/// The three step configurations, each holding one or two tables.
pub(crate) static STEPS: &[&[&[(&str, &str)]]] =
    &[&[STEP1_T0, STEP1_T1], &[STEP2_T0, STEP2_T1], &[STEP3_T0]];

#[cfg(test)]
mod tests {
    use super::{STEP1_T0, STEP1_T1, STEP3_T0, STEPS};

    /// The step-1 suffixes that appear in neither of the publication's two
    /// appendix listings, whose union holds 229 distinct step-1 suffixes.
    ///
    /// Two repair an omission the paper has: it lists `eurs` without `eur`,
    /// and omits `âtes` (*vous chantâtes*)
    /// while listing every other simple-past ending. The third, `ien`, is the
    /// entry the module note records as defective for want of a plural.
    ///
    /// `euse` and `iere` were listed here until an independent check found them
    /// in the abbreviated listing (Carry2.txt lines 378 and 386), inside its run
    /// of `-e` suffixes. A list of exceptions is only worth having if each entry
    /// earned its place, so the two that had not were removed.
    const NOT_IN_THE_PUBLICATION: &[(&str, &str)] = &[("eur", ""), ("ien", "i"), ("âtes", "")];

    /// The letters no key uses, which is what makes every entry reachable.
    ///
    /// A base ending in one of these cannot be the tail of any longer key, so
    /// no entry can be permanently shadowed by a longer one — see
    /// `crate::carry::tests::every_table_entry_can_actually_fire`, which builds
    /// its witnesses on exactly this fact.
    const LETTERS_NO_KEY_USES: &[char] = &['j', 'k', 'w'];

    fn keys<'a>(table: &[(&'a str, &str)]) -> Vec<&'a str> {
        table.iter().map(|(k, _)| *k).collect()
    }

    #[test]
    fn no_key_holds_the_letters_the_reachability_witnesses_rely_on() {
        let mut offenders: Vec<String> = Vec::new();
        for (s, step) in STEPS.iter().enumerate() {
            for (t, table) in step.iter().enumerate() {
                for (key, _) in table.iter() {
                    for c in LETTERS_NO_KEY_USES {
                        if key.contains(*c) {
                            offenders.push(format!("step {s} table {t}: {key:?} holds {c:?}"));
                        }
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "a key gained one of the letters every reachability witness uses to \
             block longer keys; the reachability proof no longer holds and the \
             witness letter must change: {offenders:#?}"
        );
    }

    /// A step's two tables must not share a key, or the minimum-radix-2 copy is
    /// dead on arrival.
    ///
    /// `crate::carry::transform` tries table 0 before table 1 for the *same*
    /// suffix. Both entries carry the same replacement in practice, so the
    /// candidate string is identical and the two size tests are `> 0` and
    /// `> 1`: whenever the stricter one would pass, the looser one has already
    /// returned. A shared key can therefore never reach table 1.
    #[test]
    fn the_two_tables_of_a_step_share_no_key() {
        let mut shared: Vec<String> = Vec::new();
        for (s, step) in STEPS.iter().enumerate() {
            if let [t0, t1] = step {
                for k in keys(t0) {
                    if keys(t1).contains(&k) {
                        shared.push(format!("step {s}: {k:?}"));
                    }
                }
            }
        }
        assert!(
            shared.is_empty(),
            "{} suffixes are listed in both tables of one step, so the \
             minimum-radix-2 copy can never fire: {shared:#?}",
            shared.len()
        );
    }

    /// No replacement is longer than the suffix it replaces, so no stem is ever
    /// longer than the word it came from.
    ///
    /// Two entries replace a suffix with something exactly as long — `f -> v`
    /// and `yeux -> oeil` — and both are recoding rules rather than removals.
    /// Anything *longer* would mean a step could grow a word, which no step of
    /// Carry is supposed to be able to do.
    #[test]
    fn no_replacement_is_longer_than_the_suffix_it_replaces() {
        let mut grown: Vec<String> = Vec::new();
        let mut same_length: Vec<&str> = Vec::new();
        for (s, step) in STEPS.iter().enumerate() {
            for (t, table) in step.iter().enumerate() {
                for (key, replacement) in table.iter() {
                    let (k, r) = (key.chars().count(), replacement.chars().count());
                    if r > k {
                        grown.push(format!("step {s} table {t}: {key:?} -> {replacement:?}"));
                    } else if r == k {
                        same_length.push(key);
                    }
                }
            }
        }
        assert!(
            grown.is_empty(),
            "{} entries replace a suffix with something longer, so a stem could \
             come out longer than its input: {grown:#?}",
            grown.len()
        );
        assert_eq!(
            same_length,
            ["f", "yeux"],
            "the set of recoding entries that keep the word's length changed"
        );
    }

    /// Every step-1 suffix the publication does not list is one of those named
    /// above.
    ///
    /// **This pin is arithmetic, and arithmetic cannot see a content change.**
    /// It counts keys; it does not compare replacements. Changing `oise -> o`
    /// to `oise -> ""` — the paper's other, contested reading — leaves it green,
    /// as does swapping a published key for an unpublished one of the same
    /// count. An entry-by-entry comparison against a transcribed listing, the
    /// way [`super::lancaster_rules`] pins Paice/Husk, is what would close that
    /// and is not yet written.
    #[test]
    fn step_one_holds_only_the_named_unpublished_additions() {
        for (key, replacement) in NOT_IN_THE_PUBLICATION {
            assert!(
                STEP1_T0.contains(&(*key, *replacement)),
                "{key:?} -> {replacement:?} is recorded as an addition to the \
                 published rules but is no longer in STEP1_T0; the module note \
                 and this list have to move together"
            );
        }
        // 228 of the union's 229: `îtés` appears in the abbreviated listing
        // only and is not in these tables. Counted here rather than left
        // implicit, so a future edit cannot close the gap without saying so.
        assert_eq!(
            STEP1_T0.len() + STEP1_T1.len() - NOT_IN_THE_PUBLICATION.len(),
            228,
            "step 1 no longer holds 228 of the publication's 229 suffixes plus \
             the {} named additions",
            NOT_IN_THE_PUBLICATION.len()
        );
    }

    /// Step 3 diverges from the publication in exactly four places.
    ///
    /// The paper's step 3 is seven rules: `nn ll tt y t qu gu`. This table adds
    /// the three remaining doubled consonants `mm`, `pp` and `ss` — which the
    /// paper's `nn`/`ll`/`tt` plainly wanted — and omits `gu -> g`, so `aigu`
    /// keeps its `u` where the paper would have cut it. Both divergences are
    /// shared with every reference implementation.
    #[test]
    fn step_three_diverges_from_the_publication_in_exactly_four_places() {
        let published: &[(&str, &str)] = &[
            ("ll", "l"),
            ("nn", "n"),
            ("qu", "c"),
            ("t", ""),
            ("tt", "t"),
            ("y", ""),
        ];
        let added: &[(&str, &str)] = &[("mm", "m"), ("pp", "p"), ("ss", "s")];
        for entry in published.iter().chain(added) {
            assert!(
                STEP3_T0.contains(entry),
                "step 3 lost {entry:?}, which the module note says it holds"
            );
        }
        assert!(
            !STEP3_T0.iter().any(|(k, _)| *k == "gu"),
            "step 3 gained the publication's `gu -> g`; that is a real repair, \
             but the module note and this test have to say so"
        );
        assert_eq!(
            STEP3_T0.len(),
            published.len() + added.len(),
            "step 3 gained or lost an entry outside the four divergences this \
             test enumerates"
        );
    }
}
