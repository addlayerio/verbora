//! `ngrams` is `slice::windows` with the zero-size precondition discharged by
//! the type. These tests pin that claim, the emission order, and the answer
//! for `n` larger than the sequence.
//!
//! Every expected value here is derived from the definition in
//! `docs/design/text-shaping-contract.md` §3.3 or computed inline; none was
//! produced by running the code.

use std::num::NonZeroUsize;

use verbora_ngrams::ngrams;

/// `NonZeroUsize` from a value the test knows to be non-zero.
fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("test widths are non-zero")
}

#[test]
fn ngrams_is_slice_windows_over_a_sweep() {
    // `slice::windows` is the specification `ngrams` claims to implement, so
    // it is the oracle. The sweep reaches `n > len` and `len == 0`, which is
    // where a hand-written windowing loop goes wrong.
    let full: Vec<usize> = (0..16).collect();
    for len in 0..=16usize {
        let seq = &full[..len];
        for n in 1..=20usize {
            assert!(
                ngrams(seq, nz(n)).eq(seq.windows(n)),
                "len {len}, n {n}: ngrams disagrees with slice::windows"
            );
        }
    }
}

#[test]
fn a_window_wider_than_the_sequence_yields_nothing() {
    // The contract: "n > seq.len() yields an empty iterator".
    let seq = ["a", "b", "c"];
    for n in 4..=8usize {
        let mut it = ngrams(&seq, nz(n));
        assert_eq!(it.len(), 0, "n {n} should yield no windows");
        assert_eq!(it.next(), None, "n {n} should yield no windows");
    }
    // And an empty sequence has no window of any width, including n == 1.
    let empty: [&str; 0] = [];
    for n in 1..=8usize {
        assert_eq!(ngrams(&empty, nz(n)).len(), 0, "empty sequence, n {n}");
    }
}

#[test]
fn every_window_holds_exactly_n_elements_and_the_count_is_len_minus_n_plus_one() {
    let full: Vec<usize> = (0..12).collect();
    for len in 0..=12usize {
        let seq = &full[..len];
        for n in 1..=12usize {
            // Written from the formula in the contract, not read off the code.
            let expected = if len >= n { len - n + 1 } else { 0 };
            let windows: Vec<&[usize]> = ngrams(seq, nz(n)).collect();
            assert_eq!(windows.len(), expected, "len {len}, n {n}");
            assert!(
                windows.iter().all(|w| w.len() == n),
                "len {len}, n {n}: a window did not hold exactly n elements"
            );
        }
    }
}

#[test]
fn windows_are_emitted_in_left_to_right_position_order() {
    // Elements are distinct and increasing, so a window's first element is its
    // own start position. A test over repeated elements could not see a
    // reordering at all — which is exactly how the replaced implementation
    // emitted a trailing tuple before the one that should precede it.
    let seq: Vec<usize> = (0..10).collect();
    for n in 1..=10usize {
        for (i, window) in ngrams(&seq, nz(n)).enumerate() {
            assert_eq!(window[0], i, "n {n}: window {i} starts at the wrong place");
            assert_eq!(
                window,
                &seq[i..i + n],
                "n {n}: window {i} has wrong content"
            );
        }
    }
}

#[test]
fn windows_borrow_from_the_caller_slice() {
    // The contract's zero-copy claim: window `i` starts at the address of
    // element `i` of the caller's own slice, so it is a borrow at an offset
    // and never a copy. Pointer identity is checked rather than equality,
    // because equality holds for a copy too.
    let seq: Vec<String> = (0..8).map(|i| i.to_string()).collect();
    for (i, window) in ngrams(&seq, nz(3)).enumerate() {
        assert!(
            std::ptr::eq(window.as_ptr(), &raw const seq[i]),
            "window {i} is not a borrow of the input at offset {i}"
        );
    }
}

#[test]
fn the_element_type_is_the_callers() {
    // The crate chooses no unit: whatever the slice holds is the n-gram unit.
    let bytes = [1_u8, 2, 3];
    assert_eq!(ngrams(&bytes, nz(2)).collect::<Vec<_>>(), [[1, 2], [2, 3]]);

    let pairs = [(1, 'a'), (2, 'b'), (3, 'c')];
    assert_eq!(ngrams(&pairs, nz(2)).len(), 2);

    let units = [(), (), ()];
    assert_eq!(ngrams(&units, nz(2)).len(), 2);

    let nested: Vec<Vec<u8>> = vec![vec![1], vec![2], vec![3]];
    assert_eq!(ngrams(&nested, nz(3)).len(), 1);
}

#[test]
fn counting_n_grams_is_a_fold_keyed_on_the_n_gram() {
    // The replacement for the deleted `NGramStats`, as the crate rustdoc shows
    // it. The key is the n-gram itself, so `["a, b"]` and `["a", "b"]` — which
    // the deleted `ngram_key` rendered identically — cannot collide.
    use std::collections::HashMap;

    let tokens = ["a, b", "x"];
    let joined = ["a", "b"];
    let n = nz(1);

    let mut counts: HashMap<&[&str], u64> = HashMap::new();
    for window in ngrams(&tokens, n) {
        *counts.entry(window).or_default() += 1;
    }
    for window in ngrams(&joined, n) {
        *counts.entry(window).or_default() += 1;
    }

    assert_eq!(counts[&["a, b"][..]], 1);
    assert_eq!(counts[&["a"][..]], 1);
    assert_eq!(counts[&["b"][..]], 1);
    assert_eq!(counts[&["x"][..]], 1);
    // An n-gram that does not occur is absent, not zero.
    assert_eq!(counts.get(&["z"][..]), None);
}
