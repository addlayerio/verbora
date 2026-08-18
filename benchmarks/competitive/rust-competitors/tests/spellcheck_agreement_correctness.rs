//! TEMPORARY PROBE — will be rewritten. Measures agreement rates.

use fast_symspell::{
    AsciiStringStrategy as FastAsciiStringStrategy, SymSpell as FastSymSpell,
    SymSpellBuilder as FastSymSpellBuilder, Verbosity as FastVerbosity,
};
use harper_core::spell::{Dictionary as HarperDictionary, FstDictionary};
use harper_core::{CharString, DictWordMetadata};
use symspell::{AsciiStringStrategy, SymSpell, SymSpellBuilder, Verbosity};
use verbora_spellcheck::{FuzzyIndexBuilder, Spellcheck};

fn load_words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join("benches/data/words.json");
    let body = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    json["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap().to_owned())
        .collect()
}

fn distinct(words: &[String], n: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    words
        .iter()
        .filter(|w| seen.insert((*w).clone()))
        .take(n)
        .cloned()
        .collect()
}

fn build_symspell(words: &[String], d: i64) -> SymSpell<AsciiStringStrategy> {
    let mut sc: SymSpell<AsciiStringStrategy> = SymSpellBuilder::default()
        .max_dictionary_edit_distance(d)
        .build()
        .unwrap();
    for w in words {
        sc.load_dictionary_line(&format!("{w} 1"), 0, 1, " ");
    }
    sc
}

fn build_fast(words: &[String], d: i64) -> FastSymSpell<FastAsciiStringStrategy> {
    let mut sc: FastSymSpell<FastAsciiStringStrategy> = FastSymSpellBuilder::default()
        .max_dictionary_edit_distance(d)
        .count_threshold(0)
        .build()
        .unwrap();
    for w in words {
        sc.load_dictionary_line(&format!("{w} 1"), 0, 1, " ");
    }
    sc
}

fn build_harper(words: &[String]) -> FstDictionary {
    let entries: Vec<(CharString, DictWordMetadata)> = words
        .iter()
        .map(|w| {
            (
                w.chars().collect::<CharString>(),
                DictWordMetadata::default(),
            )
        })
        .collect();
    FstDictionary::new(entries)
}

fn typo_del(word: &str) -> String {
    let mut c: Vec<char> = word.chars().collect();
    if c.len() > 1 {
        c.remove(c.len() / 2);
    }
    c.into_iter().collect()
}

fn typo_sub(word: &str) -> String {
    let mut c: Vec<char> = word.chars().collect();
    let i = c.len() / 2;
    c[i] = if c[i] == 'q' { 'x' } else { 'q' };
    c.into_iter().collect()
}

fn typo_ins_dup(word: &str) -> String {
    // insert a duplicate of the middle char right after itself+1 position
    let mut c: Vec<char> = word.chars().collect();
    let i = c.len() / 2;
    let ch = c[i];
    c.insert((i + 2).min(c.len()), ch);
    c.into_iter().collect()
}

fn typo_swap(word: &str) -> Option<String> {
    let mut c: Vec<char> = word.chars().collect();
    let i = c.len() / 2;
    if i + 1 >= c.len() || c[i] == c[i + 1] {
        return None;
    }
    c.swap(i, i + 1);
    Some(c.into_iter().collect())
}

#[test]
fn probe_spellbook_suggest() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .join("models/hunspell-en_US");
    if !dir.join("en_US.aff").is_file() {
        println!("no dict");
        return;
    }
    let aff = std::fs::read_to_string(dir.join("en_US.aff")).unwrap();
    let dic = std::fs::read_to_string(dir.join("en_US.dic")).unwrap();
    let dict = spellbook::Dictionary::new(&aff, &dic).unwrap();
    for (typo, want) in [
        ("helo", "hello"),
        ("wrold", "world"),
        ("recieve", "receive"),
        ("tommorow", "tomorrow"),
        ("definately", "definitely"),
        ("occured", "occurred"),
        ("seperate", "separate"),
        ("beleive", "believe"),
        ("korrect", "correct"),
        ("langage", "language"),
        ("dictionry", "dictionary"),
        ("performence", "performance"),
    ] {
        let mut out = Vec::new();
        dict.suggest(typo, &mut out);
        println!(
            "{typo}: want={want} got_want={} all={:?}",
            out.iter().any(|s| s == want),
            &out[..out.len().min(5)]
        );
    }
    for w in [
        "keyboard",
        "algorithm",
        "paragraph",
        "reliable",
        "sentence",
        "quality",
    ] {
        println!("check {w}: {}", dict.check(w));
    }
    for w in ["zqhello", "qzworld", "xxplaygroundzz", "vvvbenchmark"] {
        println!("miss_far {w}: {}", dict.check(w));
    }
}

#[test]
fn probe_verbora_symspell_harper_d1() {
    let corpus = distinct(&load_words(), 2000);
    let corpus_set: std::collections::HashSet<&str> = corpus.iter().map(String::as_str).collect();
    let sc = Spellcheck::new(corpus.iter().cloned());
    let sym = build_symspell(&corpus, 2);
    let harper = build_harper(&corpus);

    for (kind, mk) in [
        ("del", typo_del as fn(&str) -> String),
        ("sub", typo_sub as fn(&str) -> String),
        ("ins", typo_ins_dup as fn(&str) -> String),
    ] {
        let mut n = 0;
        let mut vs_sym = 0;
        let mut vs_harper = 0;
        let mut examples = Vec::new();
        for w in corpus.iter().filter(|w| w.chars().count() > 2).take(300) {
            let probe = mk(w);
            let mut v: Vec<String> = sc.get_corrections(&probe, 1);
            v.sort();
            v.dedup();
            let mut s: Vec<String> = sym
                .lookup(&probe, Verbosity::All, 1)
                .into_iter()
                .map(|x| x.term)
                .collect();
            s.sort();
            s.dedup();
            let mut h: Vec<String> = harper
                .fuzzy_match_str(&probe, 1, usize::MAX)
                .into_iter()
                .map(|r| r.word.iter().collect::<String>())
                .collect();
            if corpus_set.contains(probe.as_str()) {
                h.push(probe.clone());
            }
            h.sort();
            h.dedup();
            n += 1;
            if v == s {
                vs_sym += 1;
            } else if examples.len() < 3 {
                examples.push(format!("SYM probe={probe} v={v:?} s={s:?}"));
            }
            if v == h {
                vs_harper += 1;
            } else if examples.len() < 6 {
                examples.push(format!("HARP probe={probe} v={v:?} h={h:?}"));
            }
        }
        println!("kind={kind} n={n} verbora==symspell {vs_sym} verbora==harper {vs_harper}");
        for e in &examples {
            println!("  {e}");
        }
    }

    // swap typos
    let mut n = 0;
    let mut vs_sym = 0;
    let mut vs_harper = 0;
    for w in corpus.iter().filter(|w| w.chars().count() > 2).take(300) {
        let Some(probe) = typo_swap(w) else { continue };
        let mut v: Vec<String> = sc.get_corrections(&probe, 1);
        v.sort();
        v.dedup();
        let mut s: Vec<String> = sym
            .lookup(&probe, Verbosity::All, 1)
            .into_iter()
            .map(|x| x.term)
            .collect();
        s.sort();
        s.dedup();
        let mut h: Vec<String> = harper
            .fuzzy_match_str(&probe, 1, usize::MAX)
            .into_iter()
            .map(|r| r.word.iter().collect::<String>())
            .collect();
        if corpus_set.contains(probe.as_str()) {
            h.push(probe.clone());
        }
        h.sort();
        h.dedup();
        n += 1;
        if v == s {
            vs_sym += 1;
        }
        if v == h {
            vs_harper += 1;
        }
    }
    println!("kind=swap n={n} verbora==symspell {vs_sym} verbora==harper {vs_harper}");
}

#[test]
fn probe_verbora_symspell_d2() {
    let corpus = distinct(&load_words(), 2000);
    let sc = Spellcheck::new(corpus.iter().cloned());
    let sym = build_symspell(&corpus, 2);
    let mut n = 0;
    let mut agree = 0;
    let mut examples = Vec::new();
    for w in corpus.iter().filter(|w| w.chars().count() > 3).take(150) {
        // two deletions
        let probe = typo_del(&typo_del(w));
        let mut v: Vec<String> = sc.get_corrections(&probe, 2);
        v.sort();
        v.dedup();
        let mut s: Vec<String> = sym
            .lookup(&probe, Verbosity::All, 2)
            .into_iter()
            .map(|x| x.term)
            .collect();
        s.sort();
        s.dedup();
        n += 1;
        if v == s {
            agree += 1;
        } else if examples.len() < 5 {
            let only_v: Vec<_> = v.iter().filter(|x| !s.contains(x)).collect();
            let only_s: Vec<_> = s.iter().filter(|x| !v.contains(x)).collect();
            examples.push(format!("probe={probe} only_v={only_v:?} only_s={only_s:?}"));
        }
    }
    println!("d2 n={n} agree={agree}");
    for e in &examples {
        println!("  {e}");
    }
}

#[test]
fn probe_d2_mixed_and_harper() {
    let corpus = distinct(&load_words(), 2000);
    let corpus_set: std::collections::HashSet<&str> = corpus.iter().map(String::as_str).collect();
    let sc = Spellcheck::new(corpus.iter().cloned());
    let sym = build_symspell(&corpus, 2);
    let harper = build_harper(&corpus);

    for (kind, mk) in [
        (
            "del_del",
            (|w: &str| Some(typo_del(&typo_del(w)))) as fn(&str) -> Option<String>,
        ),
        ("sub_del", |w: &str| Some(typo_sub(&typo_del(w)))),
        ("swap_del", |w: &str| typo_swap(&typo_del(w))),
        ("ins_ins", |w: &str| Some(typo_ins_dup(&typo_ins_dup(w)))),
    ] {
        let mut n = 0;
        let mut vs_sym = 0;
        let mut harper_subset = 0;
        let mut harper_equal = 0;
        let mut examples = Vec::new();
        for w in corpus.iter().filter(|w| w.chars().count() > 4).take(150) {
            let Some(probe) = mk(w) else { continue };
            let mut v: Vec<String> = sc.get_corrections(&probe, 2);
            v.sort();
            v.dedup();
            let mut s: Vec<String> = sym
                .lookup(&probe, Verbosity::All, 2)
                .into_iter()
                .map(|x| x.term)
                .collect();
            s.sort();
            s.dedup();
            let mut h: Vec<String> = harper
                .fuzzy_match_str(&probe, 2, usize::MAX)
                .into_iter()
                .map(|r| r.word.iter().collect::<String>())
                .collect();
            if corpus_set.contains(probe.as_str()) {
                h.push(probe.clone());
            }
            h.sort();
            h.dedup();
            n += 1;
            if v == s {
                vs_sym += 1;
            } else if examples.len() < 4 {
                let only_v: Vec<_> = v.iter().filter(|x| !s.contains(x)).collect();
                let only_s: Vec<_> = s.iter().filter(|x| !v.contains(x)).collect();
                examples.push(format!(
                    "SYM {kind} probe={probe} only_v={only_v:?} only_s={only_s:?}"
                ));
            }
            let sub = h.iter().all(|x| v.contains(x));
            if sub {
                harper_subset += 1;
            } else if examples.len() < 8 {
                let only_h: Vec<_> = h.iter().filter(|x| !v.contains(x)).collect();
                examples.push(format!("HARPNOTSUB {kind} probe={probe} only_h={only_h:?}"));
            }
            if v == h {
                harper_equal += 1;
            } else if examples.len() < 12 {
                let only_v: Vec<_> = v.iter().filter(|x| !h.contains(x)).collect();
                for x in &only_v {
                    let osa = strsim::osa_distance(&probe, x);
                    let dl = strsim::damerau_levenshtein(&probe, x);
                    if osa <= 2 {
                        examples.push(format!(
                            "HARPMISS {kind} probe={probe} v_only={x} osa={osa} dl={dl}"
                        ));
                    }
                }
            }
        }
        println!(
            "d2 kind={kind} n={n} vs_sym={vs_sym} harper_subset={harper_subset} harper_equal={harper_equal}"
        );
        for e in &examples {
            println!("  {e}");
        }
    }
}

#[test]
fn probe_fast_symspell_shapes() {
    let corpus = distinct(&load_words(), 2000);
    let fast = build_fast(&corpus, 2);
    let mut builder = FuzzyIndexBuilder::new();
    for w in &corpus {
        builder.insert(w);
    }
    let index = builder.build();

    for (kind, mk) in [
        ("sub", typo_sub as fn(&str) -> String),
        ("ins_dup", typo_ins_dup as fn(&str) -> String),
    ] {
        let mut n = 0;
        let mut agree = 0;
        let mut unexplained = 0;
        for w in corpus.iter().filter(|w| w.chars().count() > 1).take(300) {
            let probe = mk(w);
            let mut tree: Vec<&str> = index.neighbors(&probe, 1).collect();
            tree.sort_unstable();
            tree.dedup();
            let mut fh: Vec<String> = fast
                .lookup(&probe, FastVerbosity::All, 1)
                .into_iter()
                .map(|s| s.term)
                .collect();
            fh.sort();
            fh.dedup();
            let fr: Vec<&str> = fh.iter().map(String::as_str).collect();
            n += 1;
            if tree == fr {
                agree += 1;
                continue;
            }
            for t in tree.iter().filter(|w| !fr.contains(w)) {
                if strsim::damerau_levenshtein(&probe, t) != 1 {
                    unexplained += 1;
                    println!("  UNEXPLAINED tree-only {kind} probe={probe} t={t}");
                }
            }
            for f in fr.iter().filter(|w| !tree.contains(w)) {
                if strsim::damerau_levenshtein(&probe, f) != 1 {
                    unexplained += 1;
                    println!("  UNEXPLAINED fast-only {kind} probe={probe} f={f}");
                }
            }
        }
        println!("fast kind={kind} n={n} agree={agree} unexplained={unexplained}");
    }

    // transposition sweep: does fast_symspell reach every swap at d1?
    let mut n = 0;
    let mut miss = 0;
    let corpus_set: std::collections::HashSet<&str> = corpus.iter().map(String::as_str).collect();
    for w in corpus.iter().filter(|w| w.chars().count() >= 4).take(300) {
        let Some(probe) = typo_swap(w) else { continue };
        if corpus_set.contains(probe.as_str()) {
            continue;
        }
        n += 1;
        let hit = fast
            .lookup(&probe, FastVerbosity::All, 1)
            .iter()
            .any(|s| &s.term == w);
        if !hit {
            miss += 1;
            if miss <= 5 {
                println!("  fast swap miss: {probe} -> {w}");
            }
        }
    }
    println!("fast swap n={n} miss={miss}");

    // rdamerau candidate counterexample shapes
    for (a, b) in [
        ("aba", "abba"),
        ("abc", "abca"),
        ("abc", "aabc"),
        ("abcd", "abcbd"),
        ("hello", "helllo"),
        ("hello", "heallo"),
        ("commite", "committe"),
        ("recieve", "receieve"),
        ("banana", "bananna"),
        ("abcde", "abcdce"),
        ("abcde", "abacde"),
        ("xyz", "xyzy"),
        ("tac", "tatc"),
        ("xyz", "xyxz"),
    ] {
        let ta = triple_accel::levenshtein::rdamerau_exp(a.as_bytes(), b.as_bytes());
        let ss = strsim::damerau_levenshtein(a, b);
        let pl = triple_accel::levenshtein::levenshtein_exp(a.as_bytes(), b.as_bytes());
        println!("rdamerau {a} -> {b}: triple={ta} strsim={ss} plain={pl}");
    }
}
