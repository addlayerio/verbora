//! `Padded`, from the definition in `docs/design/text-shaping-contract.md`
//! §3.3: `k = n - 1` copies of each supplied symbol, symmetric, with every
//! window exactly `n` elements wide and the window count
//! `len + k_start + k_end - n + 1` or zero.
//!
//! Every expected value is derived from that definition or computed inline by
//! an independently written oracle. None was produced by running the code.

use std::num::NonZeroUsize;

use verbora_ngrams::Padded;

/// `NonZeroUsize` from a value the test knows to be non-zero.
fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("test widths are non-zero")
}

/// The padded sequence, built from the definition rather than from the crate.
fn oracle_sequence<'a>(
    seq: &[&'a str],
    n: usize,
    start: Option<&'a str>,
    end: Option<&'a str>,
) -> Vec<&'a str> {
    let k = n - 1;
    let mut out = Vec::new();
    if let Some(symbol) = start {
        for _ in 0..k {
            out.push(symbol);
        }
    }
    out.extend_from_slice(seq);
    if let Some(symbol) = end {
        for _ in 0..k {
            out.push(symbol);
        }
    }
    out
}

/// The window count, written from the formula and not from any iterator.
fn oracle_count(len: usize, n: usize, start: bool, end: bool) -> usize {
    let k = n - 1;
    let k_start = if start { k } else { 0 };
    let k_end = if end { k } else { 0 };
    let padded = len + k_start + k_end;
    if padded >= n { padded - n + 1 } else { 0 }
}

/// Windows of `n` over `seq`, by index arithmetic rather than `slice::windows`.
fn oracle_windows<'a>(seq: &[&'a str], n: usize) -> Vec<Vec<&'a str>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + n <= seq.len() {
        out.push(seq[i..i + n].to_vec());
        i += 1;
    }
    out
}

/// The four padding combinations, as `(start, end)`.
const COMBINATIONS: [(Option<&str>, Option<&str>); 4] = [
    (None, None),
    (Some("S"), None),
    (None, Some("E")),
    (Some("S"), Some("E")),
];

#[test]
fn the_sweep_matches_the_definition_in_shape_count_and_content() {
    // len 0..=8 × n 1..=8 × all four combinations, which is the sweep §6.6
    // asks for. It reaches `n > len` and `len == 0`, the two classes where the
    // replaced implementation produced short and out-of-order tuples.
    let corpus = ["a", "b", "c", "d", "e", "f", "g", "h"];

    for len in 0..=8usize {
        let seq = &corpus[..len];
        for n in 1..=8usize {
            for (start, end) in COMBINATIONS {
                let label = format!("len {len}, n {n}, start {start:?}, end {end:?}");

                let padded = Padded::new(seq, nz(n), start.as_ref(), end.as_ref());
                let expected_sequence = oracle_sequence(seq, n, start, end);
                assert_eq!(padded.as_slice(), expected_sequence.as_slice(), "{label}");

                let windows: Vec<&[&str]> = padded.ngrams().collect();

                // 1. Every window has exactly n elements.
                assert!(
                    windows.iter().all(|w| w.len() == n),
                    "{label}: a window did not hold exactly n elements"
                );

                // 2. The count matches the formula.
                let expected_count = oracle_count(len, n, start.is_some(), end.is_some());
                assert_eq!(windows.len(), expected_count, "{label}: window count");
                assert_eq!(padded.ngrams().len(), expected_count, "{label}: len()");

                // 3. The windows are the right ones, in the right order.
                let expected_windows = oracle_windows(&expected_sequence, n);
                assert_eq!(windows.len(), expected_windows.len(), "{label}");
                for (got, want) in windows.iter().zip(&expected_windows) {
                    assert_eq!(got, &want.as_slice(), "{label}: window content or order");
                }
            }
        }
    }
}

#[test]
fn three_elements_at_n_five_with_both_symbols() {
    // The contract's worked fixture. 3 + 4 + 4 = 11 elements, so
    // 11 - 5 + 1 = 7 windows of five — which is also `len + n - 1`.
    let padded = Padded::new(&["a", "b", "c"], nz(5), Some(&"<s>"), Some(&"</s>"));

    assert_eq!(
        padded.as_slice(),
        [
            "<s>", "<s>", "<s>", "<s>", "a", "b", "c", "</s>", "</s>", "</s>", "</s>"
        ]
    );

    let windows: Vec<&[&str]> = padded.ngrams().collect();
    assert_eq!(windows.len(), 3 + 5 - 1);
    assert!(windows.iter().all(|w| w.len() == 5));
    assert_eq!(windows[0], ["<s>", "<s>", "<s>", "<s>", "a"]);
    assert_eq!(windows[6], ["c", "</s>", "</s>", "</s>", "</s>"]);

    // Order matters and is the thing the replaced implementation broke: it
    // emitted `["c", "</s>"]` before the tuple that should precede it. Pin the
    // whole sequence, not just the ends.
    assert_eq!(
        windows,
        [
            ["<s>", "<s>", "<s>", "<s>", "a"],
            ["<s>", "<s>", "<s>", "a", "b"],
            ["<s>", "<s>", "a", "b", "c"],
            ["<s>", "a", "b", "c", "</s>"],
            ["a", "b", "c", "</s>", "</s>"],
            ["b", "c", "</s>", "</s>", "</s>"],
            ["c", "</s>", "</s>", "</s>", "</s>"],
        ]
    );
}

#[test]
fn an_empty_sequence_at_n_four_with_both_symbols() {
    // The contract's second fixture: three windows of four, drawn from
    // [S, S, S, E, E, E]. Nothing here comes from the sequence, because there
    // is no sequence — 0 + 3 + 3 = 6 elements, 6 - 4 + 1 = 3 windows.
    let empty: [&str; 0] = [];
    let padded = Padded::new(&empty, nz(4), Some(&"S"), Some(&"E"));

    assert_eq!(padded.as_slice(), ["S", "S", "S", "E", "E", "E"]);
    let windows: Vec<&[&str]> = padded.ngrams().collect();
    assert_eq!(
        windows,
        [
            ["S", "S", "S", "E"],
            ["S", "S", "E", "E"],
            ["S", "E", "E", "E"],
        ]
    );
}

#[test]
fn unigrams_add_no_padding_even_when_both_symbols_are_supplied() {
    // k = n - 1 = 0, so zero copies of each symbol. The symbols are not
    // discarded by a special case; the formula simply asks for none of them.
    let padded = Padded::new(&["a", "b", "c"], nz(1), Some(&"<s>"), Some(&"</s>"));
    assert_eq!(padded.as_slice(), ["a", "b", "c"]);

    let windows: Vec<&[&str]> = padded.ngrams().collect();
    assert_eq!(windows, [["a"], ["b"], ["c"]]);

    // Including on an empty sequence, where the padded form is still empty.
    let empty: [&str; 0] = [];
    let padded = Padded::new(&empty, nz(1), Some(&"<s>"), Some(&"</s>"));
    assert!(padded.as_slice().is_empty());
    assert_eq!(padded.ngrams().len(), 0);
}

#[test]
fn the_two_symbols_are_independent_options() {
    // Not one policy with four spellings: a start symbol pads the start and
    // nothing else, and an end symbol pads the end and nothing else.
    let seq = ["a", "b"];

    assert_eq!(
        Padded::new(&seq, nz(3), Some(&"S"), None).as_slice(),
        ["S", "S", "a", "b"]
    );
    assert_eq!(
        Padded::new(&seq, nz(3), None, Some(&"E")).as_slice(),
        ["a", "b", "E", "E"]
    );
    assert_eq!(Padded::new(&seq, nz(3), None, None).as_slice(), ["a", "b"]);
}

#[test]
fn the_first_and_last_element_each_appear_in_exactly_n_windows() {
    // This is the property the symmetric padding rule exists to give, and the
    // reason `n - 1` end symbols rather than one. Elements are distinct so a
    // count is unambiguous.
    let seq = ["a", "b", "c", "d"];
    for n in 1..=6usize {
        let padded = Padded::new(&seq, nz(n), Some(&"<s>"), Some(&"</s>"));
        let first = padded.ngrams().filter(|w| w.contains(&"a")).count();
        let last = padded.ngrams().filter(|w| w.contains(&"d")).count();
        assert_eq!(first, n, "n {n}: first element");
        assert_eq!(last, n, "n {n}: last element");
    }
}

#[test]
fn an_empty_pad_symbol_still_pads() {
    // A symbol is supplied or it is not; emptiness of the symbol is not a
    // second, hidden gate. `Option` carries the whole decision.
    let padded = Padded::new(&["a", "b"], nz(2), Some(&""), None);
    assert_eq!(padded.as_slice(), ["", "a", "b"]);
    let windows: Vec<&[&str]> = padded.ngrams().collect();
    assert_eq!(windows, [["", "a"], ["a", "b"]]);
}

#[test]
fn padded_holds_the_callers_element_type() {
    // No unit is imposed here either.
    let padded = Padded::new(&[1_u8, 2], nz(3), None, Some(&0));
    assert_eq!(padded.as_slice(), [1, 2, 0, 0]);
    assert_eq!(padded.ngrams().len(), 2);

    let owned: Vec<String> = ["x", "y"].iter().map(|s| (*s).to_owned()).collect();
    let pad = "<pad>".to_owned();
    let padded = Padded::new(&owned, nz(2), Some(&pad), Some(&pad));
    assert_eq!(padded.as_slice().len(), 4);
    assert_eq!(padded.ngrams().len(), 3);
}

#[test]
fn padded_is_comparable_and_cloneable() {
    let a = Padded::new(&["a", "b"], nz(2), Some(&"S"), None);
    let b = Padded::new(&["a", "b"], nz(2), Some(&"S"), None);
    assert_eq!(a, b);
    assert_eq!(a, a.clone());

    // `n` is part of the state, so two `Padded` with the same buffer but
    // different window widths are different values.
    let same_buffer_wider = Padded::new(&["S", "a", "b"], nz(3), None, None);
    assert_eq!(a.as_slice(), same_buffer_wider.as_slice());
    assert_ne!(a, same_buffer_wider);
}

// --- Totality -------------------------------------------------------------
//
// `Padded::new` materialises the padded sequence, so a large `n` asks it for a
// buffer that cannot exist. The contract requires that this be an empty
// `Padded` rather than a panic, in debug and in release.

#[test]
fn a_padded_length_that_overflows_usize_yields_an_empty_padded() {
    // len 3, k = usize::MAX - 1: the addition overflows on any side that pads.
    let seq = ["a", "b", "c"];
    for (start, end) in COMBINATIONS {
        let padded = Padded::new(&seq, NonZeroUsize::MAX, start.as_ref(), end.as_ref());
        assert_eq!(padded.ngrams().len(), 0, "start {start:?}, end {end:?}");
        assert_eq!(padded.ngrams().count(), 0, "start {start:?}, end {end:?}");
        if start.is_none() && end.is_none() {
            // Nothing overflowed here: the buffer is the sequence itself, and
            // it simply holds no window of width usize::MAX.
            assert_eq!(padded.as_slice(), ["a", "b", "c"]);
        } else {
            assert!(padded.as_slice().is_empty(), "start {start:?}, end {end:?}");
        }
    }
}

#[test]
fn a_padded_length_that_fits_usize_but_not_memory_yields_an_empty_padded() {
    // The class the overflow test above cannot reach. With len 1 and one pad
    // symbol the padded length is 1 + (usize::MAX - 1) = usize::MAX exactly,
    // so the checked addition succeeds and the reservation is what must
    // refuse. Without that second guard this call aborts on a capacity
    // overflow inside `Vec`.
    for (start, end) in [(Some("S"), None), (None, Some("E"))] {
        let padded = Padded::new(&["a"], NonZeroUsize::MAX, start.as_ref(), end.as_ref());
        assert!(padded.as_slice().is_empty(), "start {start:?}, end {end:?}");
        assert_eq!(padded.ngrams().len(), 0, "start {start:?}, end {end:?}");
    }

    // The same shape at a length that is merely absurd rather than maximal:
    // 2^60 pointers is ~18 EiB and cannot be reserved on any machine.
    let n = nz((1_usize << 60) + 1);
    let padded = Padded::new(&["a", "b"], n, Some(&"S"), Some(&"E"));
    assert!(padded.as_slice().is_empty());
    assert_eq!(padded.ngrams().len(), 0);
}

#[test]
fn a_zero_sized_element_type_is_refused_at_every_padded_length_it_cannot_build() {
    // §6.6's named class. A zero-sized element type is the one case where
    // nothing downstream of `Padded::new` can refuse on its behalf: no
    // reservation of `()` fails on its own, so the buffer would be accepted and
    // then filled one no-op write at a time. Both of `new`'s guards therefore
    // have to hold, and this walks every case in which one of them must fire.
    //
    // At `n = usize::MAX` the pad count is `k = usize::MAX - 1`, so for a
    // sequence of `len` zero-sized elements the padded length is:
    //
    // | pads     | padded length            | len 0 | len 1 | len 2, 3, 4 |
    // |----------|--------------------------|-------|-------|-------------|
    // | one      | `len + (usize::MAX - 1)` | fits  | fits  | overflows   |
    // | both     | `len + 2*(usize::MAX-1)` | ovf   | ovf   | overflows   |
    // | neither  | `len`                    | fits  | fits  | fits        |
    //
    // "fits" and "overflows" name which guard answers, not whether the call is
    // refused: a padded length that fits `usize` is still `usize::MAX`-ish
    // elements, so the one-byte charge in the reservation refuses it. Only the
    // unpadded row builds a buffer, and it builds the sequence itself.
    //
    // Nothing here needs a sequence of astronomical length to reach the
    // overflow: `checked_add` is symmetric in `seq.len()` and the pad count, so
    // a huge `n` over a two-element sequence overflows it in exactly the same
    // addition a huge sequence would. Expressing the other operand instead
    // would take a `[(); 2^63]`, and 2^63 does not fit the signed 64-bit count
    // DWARF gives an array subrange — that type breaks the build under debug
    // info on some targets, whether or not any test ever uses it.
    const PADS: [(Option<&()>, Option<&()>); 4] = [
        (None, None),
        (Some(&()), None),
        (None, Some(&())),
        (Some(&()), Some(&())),
    ];

    let seq = [(); 4];
    for len in 0..=seq.len() {
        let seq = &seq[..len];
        for (start, end) in PADS {
            let padded = Padded::new(seq, NonZeroUsize::MAX, start, end);
            let label = format!(
                "len {len}, start {}, end {}",
                start.is_some(),
                end.is_some()
            );

            // A window of `usize::MAX` elements exists over no buffer this
            // machine holds, so the window set is empty either way; what
            // separates the rows is the buffer.
            assert_eq!(padded.ngrams().len(), 0, "{label}");
            assert_eq!(padded.ngrams().count(), 0, "{label}");

            if start.is_none() && end.is_none() {
                assert_eq!(padded.as_slice().len(), len, "{label}");
            } else {
                assert!(padded.as_slice().is_empty(), "{label}");
            }
        }
    }
}

#[test]
fn a_zero_sized_element_type_is_refused_at_a_length_it_could_never_finish_filling() {
    // The row of the grid above that makes `new` total but *not terminating*,
    // pulled out because what it asserts is termination rather than an answer:
    // a padded length that fits `usize` and belongs to an element type whose
    // reservation cannot fail. `[(); 0]` with `n = usize::MAX` asks for
    // `usize::MAX - 1` pad copies — the checked addition succeeds, because
    // nothing overflows, and `Vec<()>` reserves any capacity at all, because
    // zero-sized elements occupy no memory. Neither refusal fires, and the
    // buffer is then filled one element at a time.
    //
    // `usize::MAX - 1` no-op writes is not a running cost, it is a hang: this
    // call did not return before the refusal that charges a zero-sized element
    // one byte was added, so this test does not merely assert an answer, it
    // asserts that there is one.
    let empty: [(); 0] = [];
    for (start, end) in [(Some(&()), None), (None, Some(&())), (Some(&()), Some(&()))] {
        let padded = Padded::new(&empty, NonZeroUsize::MAX, start, end);
        assert!(padded.as_slice().is_empty(), "start {start:?}, end {end:?}");
        assert_eq!(padded.ngrams().len(), 0, "start {start:?}, end {end:?}");
        assert_eq!(padded.ngrams().count(), 0, "start {start:?}, end {end:?}");
    }

    // The same class one power of two below the ceiling, where the length is
    // merely absurd rather than maximal and the checked addition is nowhere
    // near overflowing: 2^61 elements charged one byte each is 2 EiB, which no
    // machine reserves. A guard that only rejected lengths above `isize::MAX`
    // would accept this one and spend 2^61 steps on it.
    let n = nz((1_usize << 60) + 1);
    let padded = Padded::new(&[(), ()], n, Some(&()), Some(&()));
    assert!(padded.as_slice().is_empty());
    assert_eq!(padded.ngrams().len(), 0);
}

#[test]
fn a_zero_sized_element_type_pads_normally_at_an_ordinary_length() {
    // The other side of the guard above: refusing a length that cannot be
    // filled must not turn into refusing zero-sized element types. `()` pads,
    // windows and counts exactly like any other `T: Clone`.
    let padded = Padded::new(&[(), ()], nz(3), Some(&()), Some(&()));
    assert_eq!(padded.as_slice().len(), 2 + 2 + 2);
    assert_eq!(padded.ngrams().len(), 6 - 3 + 1);
    assert!(padded.ngrams().all(|w| w.len() == 3));

    // And at a length large enough to be a real buffer, so the guard is not
    // secretly a small-length special case: 1000 elements of a type that
    // occupies no memory.
    let seq = [(); 100];
    let padded = Padded::new(&seq, nz(901), Some(&()), None);
    assert_eq!(padded.as_slice().len(), 100 + 900);
    assert_eq!(padded.ngrams().len(), 1000 - 901 + 1);
}

#[test]
fn a_huge_n_without_padding_is_simply_an_empty_window_set() {
    // Nothing overflows and nothing is refused: the buffer is a copy of the
    // sequence, and `usize::MAX` is wider than it. Zero windows here is the
    // ordinary `n > len` answer.
    //
    // The sequence is kept small on purpose. `Padded::new` copies it, so the
    // copy is O(len) — the cost the crate documents — and a 2^63-element
    // zero-sized sequence would be a test that measures how long `Vec` takes
    // to count to 2^63, not a test of this crate.
    let padded = Padded::new(&[(), (), ()], NonZeroUsize::MAX, None, None);
    assert_eq!(padded.as_slice().len(), 3);
    assert_eq!(padded.ngrams().len(), 0);
}
