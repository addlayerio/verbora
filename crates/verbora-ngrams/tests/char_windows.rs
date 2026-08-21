//! `char_ngrams`, from `docs/design/text-shaping-contract.md` §3.3 and §2.3:
//! windows of exactly `n` Unicode scalars, each a borrowed substring of the
//! input, with `ExactSizeIterator::len` equal to
//! `text.chars().count().saturating_sub(n - 1)`.
//!
//! The corpus deliberately carries astral scalars, combining marks and a
//! caller-supplied `U+FFFD`: `char_ngrams` replaces a UTF-16 code-unit
//! splitter whose only failure mode was astral input, so a BMP-only corpus
//! could not have detected it.

use std::num::NonZeroUsize;

use verbora_ngrams::char_ngrams;

/// `NonZeroUsize` from a value the test knows to be non-zero.
fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("test widths are non-zero")
}

/// Mixed-script corpus. Every entry names the class it exists to reach.
const CORPUS: [&str; 18] = [
    "",                    // empty
    "a",                   // one ASCII scalar
    "abc",                 // ASCII
    "the quick brown fox", // ASCII with spaces
    "café naïve",          // precomposed Latin-1
    "e\u{0301}f",          // decomposed: a combining mark is its own scalar
    "Ωμέγα",               // Greek
    "привет, мир",         // Cyrillic
    "مرحبا",               // Arabic
    "שלום",                // Hebrew
    "नमस्ते",                // Devanagari, with matras
    "สวัสดี",                // Thai
    "안녕하세요",          // Hangul
    "日本語のテキスト",    // CJK
    "👍你好",              // astral + CJK
    "a👨‍👩‍👧‍👦b",                // ZWJ emoji sequence: seven scalars
    "a\u{FFFD}b",          // a replacement character the caller supplied
    "a\r\nb",              // CRLF
];

#[test]
fn every_window_holds_exactly_n_scalars_and_is_a_substring_of_the_input() {
    for text in CORPUS {
        for n in 1..=6usize {
            for window in char_ngrams(text, nz(n)) {
                assert_eq!(
                    window.chars().count(),
                    n,
                    "{text:?} at n {n}: window {window:?} is not n scalars"
                );
                assert!(
                    text.contains(window),
                    "{text:?} at n {n}: window {window:?} is not a substring"
                );
                // Nothing is fabricated: a replacement character can only come
                // out if it went in. This is the whole point of the function.
                assert!(
                    !window.contains('\u{FFFD}') || text.contains('\u{FFFD}'),
                    "{text:?} at n {n}: window {window:?} invented U+FFFD"
                );
            }
        }
    }
}

#[test]
fn windows_are_the_scalar_positions_in_order() {
    // The independent oracle: collect the scalars, window the collection, and
    // rebuild each window as a `String`. It shares no code with the iterator.
    for text in CORPUS {
        for n in 1..=6usize {
            let scalars: Vec<char> = text.chars().collect();
            let expected: Vec<String> = if scalars.len() >= n {
                scalars.windows(n).map(|w| w.iter().collect()).collect()
            } else {
                Vec::new()
            };
            let got: Vec<&str> = char_ngrams(text, nz(n)).collect();
            assert_eq!(got.len(), expected.len(), "{text:?} at n {n}");
            for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
                assert_eq!(*g, e.as_str(), "{text:?} at n {n}: window {i}");
            }
        }
    }
}

#[test]
fn len_is_the_documented_formula_and_the_number_of_items_yielded() {
    for text in CORPUS {
        for n in 1..=8usize {
            // The formula the rustdoc states, computed here from `chars`.
            let expected = text.chars().count().saturating_sub(n - 1);
            let it = char_ngrams(text, nz(n));
            assert_eq!(it.len(), expected, "{text:?} at n {n}: len()");
            assert_eq!(it.size_hint(), (expected, Some(expected)));
            // And it is the number actually yielded, which is the claim that
            // `len()` alone cannot check.
            assert_eq!(
                char_ngrams(text, nz(n)).count(),
                expected,
                "{text:?} at n {n}: items yielded"
            );
        }
    }
}

#[test]
fn len_decreases_by_one_per_item() {
    for text in CORPUS {
        let mut it = char_ngrams(text, nz(2));
        let mut expected = it.len();
        while let Some(_window) = it.next() {
            expected -= 1;
            assert_eq!(it.len(), expected, "{text:?}");
        }
        assert_eq!(expected, 0, "{text:?}");
    }
}

#[test]
fn the_contract_fixtures() {
    // `char_ngrams("abc", 2)` is ["ab", "bc"].
    assert_eq!(char_ngrams("abc", nz(2)).collect::<Vec<_>>(), ["ab", "bc"]);
    // `char_ngrams("👍你好", 2)` is ["👍你", "你好"] — three scalars, two
    // windows, and the astral scalar is whole in the one that holds it.
    assert_eq!(
        char_ngrams("👍你好", nz(2)).collect::<Vec<_>>(),
        ["👍你", "你好"]
    );
    // `char_ngrams("ab", 3)` is empty.
    assert_eq!(char_ngrams("ab", nz(3)).count(), 0);
    // A single astral scalar is one unigram, not two halves and not two
    // replacement characters.
    assert_eq!(char_ngrams("👍", nz(1)).collect::<Vec<_>>(), ["👍"]);
}

#[test]
fn a_window_may_split_a_combining_sequence_and_that_is_documented() {
    // The scalar is the unit, so `n = 2` over "e" + U+0301 + "f" yields a
    // window that begins with the combining acute. A grapheme-cluster unit
    // would not — and this test is what would fail if the unit ever changed
    // silently.
    let grams: Vec<&str> = char_ngrams("e\u{0301}f", nz(2)).collect();
    assert_eq!(grams, ["e\u{0301}", "\u{0301}f"]);
    assert_eq!(grams.len(), 2);
}

#[test]
fn an_empty_or_too_short_input_yields_nothing() {
    for n in 1..=4usize {
        assert_eq!(char_ngrams("", nz(n)).count(), 0, "n {n}");
        assert_eq!(char_ngrams("", nz(n)).len(), 0, "n {n}");
    }
    for n in 2..=8usize {
        assert_eq!(char_ngrams("a", nz(n)).count(), 0, "n {n}");
    }
    // A very large `n` is the same answer, not a panic.
    assert_eq!(char_ngrams("👍你好", NonZeroUsize::MAX).count(), 0);
    assert_eq!(char_ngrams("", NonZeroUsize::MAX).len(), 0);
}

#[test]
fn n_equal_to_the_scalar_count_yields_the_whole_input_once() {
    for text in CORPUS {
        let count = text.chars().count();
        if count == 0 {
            continue;
        }
        let grams: Vec<&str> = char_ngrams(text, nz(count)).collect();
        assert_eq!(grams, [text], "{text:?}");
    }
}

#[test]
fn reversed_iteration_visits_the_same_windows_backwards() {
    for text in CORPUS {
        for n in 1..=5usize {
            let forward: Vec<&str> = char_ngrams(text, nz(n)).collect();
            let mut backward: Vec<&str> = char_ngrams(text, nz(n)).rev().collect();
            backward.reverse();
            assert_eq!(forward, backward, "{text:?} at n {n}");
        }
    }
}

#[test]
fn alternating_ends_consumes_each_window_exactly_once() {
    // The class a `.rev()`-only test misses: a `DoubleEndedIterator` whose two
    // cursors share a single counter can double-yield or skip in the middle.
    for text in CORPUS {
        for n in 1..=4usize {
            let forward: Vec<&str> = char_ngrams(text, nz(n)).collect();

            let mut it = char_ngrams(text, nz(n));
            let mut head = Vec::new();
            let mut tail = Vec::new();
            let mut from_front = true;
            loop {
                let next = if from_front {
                    it.next()
                } else {
                    it.next_back()
                };
                match next {
                    Some(w) if from_front => head.push(w),
                    Some(w) => tail.push(w),
                    None => break,
                }
                from_front = !from_front;
            }
            tail.reverse();
            head.extend(tail);
            assert_eq!(head, forward, "{text:?} at n {n}");
        }
    }
}

#[test]
fn the_iterator_is_fused() {
    let mut it = char_ngrams("abc", nz(2));
    assert_eq!(it.next(), Some("ab"));
    assert_eq!(it.next(), Some("bc"));
    for _ in 0..4 {
        assert_eq!(it.next(), None);
        assert_eq!(it.next_back(), None);
        assert_eq!(it.len(), 0);
    }
}

#[test]
fn windows_borrow_from_the_input_text() {
    // Substrings, not copies: window `i` sits inside the input's own buffer.
    let text = String::from("👍你好abc");
    let base = text.as_ptr() as usize;
    let end = base + text.len();
    for window in char_ngrams(&text, nz(2)) {
        let start = window.as_ptr() as usize;
        assert!(
            start >= base && start + window.len() <= end,
            "window {window:?} does not lie inside the input"
        );
    }
}
