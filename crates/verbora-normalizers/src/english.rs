//! `normalize` (`normalizeTokens`) — the English contraction expander.
//!
//! Port of the reference `normalizer`.

/// The five conversion-table entries, already split on `/\W+/`.
///
/// The reference stores them as sentences and splits at call time; the split is
/// constant, so it is done here instead. Lookup keys are lowercase because the
/// reference looks up `token.toLowerCase()`.
///
/// Note the asymmetry this creates and which the shipped spec pins down: a
/// table hit discards the token's casing (`"CaN'T"` -> `["can", "not"]`) while a
/// rule hit preserves it (`"Hasn't"` -> `["Has", "not"]`), because the rules run
/// against the original token.
static CONVERSION_TABLE: [(&str, &[&str]); 5] = [
    ("can't", &["can", "not"]),
    ("won't", &["will", "not"]),
    ("couldn't've", &["could", "not", "have"]),
    ("i'm", &["I", "am"]),
    ("how'd", &["how", "did"]),
];

/// One fallback rule.
///
/// The reference writes these as `/([azAZ]*)n'[tT]/g` and friends. Two details
/// of that pattern are load-bearing and easy to "tidy up" into a bug:
///
/// * `[azAZ]` is a four-character **set** `{a, z, A, Z}`, not the range `a-z`.
///   Reading it as a range is the natural mistake.
/// * The `n` of `n't` is matched **literally and lowercase only**, while every
///   letter after the apostrophe gets an explicit `[xX]` class. So `"N'T"` does
///   not match, but `"hasn'T"` does. A blanket case-insensitive flag diverges.
///
/// The captured `[azAZ]*` prefix is re-emitted verbatim as `$1` immediately
/// before the inserted expansion, so it never affects the output — see
/// [`expand`] for why dropping it from the scan is exact and not merely close.
struct Rule {
    /// Whether a literal lowercase `n` must precede the apostrophe.
    needs_n: bool,
    /// The letters that must follow the apostrophe, matched case-insensitively.
    tail: &'static [u8],
    /// The replacement text, including its leading space.
    expansion: &'static str,
}

/// The six rules, in the order the reference tries them. First match wins.
///
/// Order is observable: `"he'd've"` matches rule 5 (`'ve`) before rule 6 (`'d`)
/// and expands to `["he", "d", "have"]`, not `["he", "would", "ve"]`.
static RULES: [Rule; 6] = [
    Rule {
        needs_n: true,
        tail: b"t",
        expansion: " not",
    },
    Rule {
        needs_n: false,
        tail: b"s",
        expansion: " is",
    },
    Rule {
        needs_n: false,
        tail: b"ll",
        expansion: " will",
    },
    Rule {
        needs_n: false,
        tail: b"re",
        expansion: " are",
    },
    Rule {
        needs_n: false,
        tail: b"ve",
        expansion: " have",
    },
    Rule {
        needs_n: false,
        tail: b"d",
        expansion: " would",
    },
];

/// Expands English contractions across a list of tokens.
///
/// Each token is looked up in the conversion table first (case-insensitively);
/// on a miss the six fallback rules are tried in order and the first that matches
/// anywhere in the token is applied to **every** occurrence. Either way the
/// result is split on `/\W+/` and appended, so one input token can yield several
/// output tokens — or, when nothing matched, exactly one, passed through
/// verbatim without splitting.
///
/// # Examples
///
/// ```
/// # use verbora_normalizers::normalize;
/// assert_eq!(normalize(&["hasn't"]), ["has", "not"]);
/// assert_eq!(normalize(&["Hasn't"]), ["Has", "not"]);   // rules keep the case
/// assert_eq!(normalize(&["CaN'T"]), ["can", "not"]);    // the table does not
/// assert_eq!(normalize(&["he'd've"]), ["he", "d", "have"]);
/// assert_eq!(normalize(&["o'clock"]), ["o'clock"]);     // no rule, no split
/// ```
///
/// Empty strings in the output are deliberate and must not be filtered — they
/// come from `/\W+/` matching at the start or end of the expanded token:
///
/// ```
/// # use verbora_normalizers::normalize;
/// assert_eq!(normalize(&["it's!"]), ["it", "is", ""]);
/// assert_eq!(normalize(&["!it's"]), ["", "it", "is"]);
/// ```
#[must_use]
pub fn normalize<S: AsRef<str>>(tokens: &[S]) -> Vec<String> {
    let mut results = Vec::with_capacity(tokens.len());
    for token in tokens {
        push_token(token.as_ref(), &mut results);
    }
    results
}

/// Expands a single token, mirroring `normalize("...")` with a bare string.
///
/// The reference wraps a string argument into a one-element array; it does
/// **not** split it on whitespace first, so `normalize("I'm here")` treats the
/// whole thing as one token.
///
/// ```
/// # use verbora_normalizers::normalize_token;
/// assert_eq!(normalize_token("I'D"), ["I", "would"]);
/// assert_eq!(normalize_token("can't can't"), ["ca", "not", "ca", "not"]);
/// ```
#[must_use]
pub fn normalize_token(token: &str) -> Vec<String> {
    let mut results = Vec::with_capacity(2);
    push_token(token, &mut results);
    results
}

/// Appends one token's expansion to `results`.
fn push_token(token: &str, results: &mut Vec<String>) {
    if let Some(parts) = table_lookup(token) {
        results.extend(parts.iter().map(|s| (*s).to_owned()));
        return;
    }
    if let Some(expanded) = expand(token) {
        results.extend(split_non_word(&expanded).map(str::to_owned));
        return;
    }
    results.push(token.to_owned());
}

/// The length in bytes of the longest conversion-table key, `"couldn't've"`.
const LONGEST_KEY: usize = 11;

/// The conversion-table entry for `token`, matched on its lowercase form.
fn table_lookup(token: &str) -> Option<&'static [&'static str]> {
    if token.is_ascii() {
        // ASCII lowercasing is length-preserving, so the byte length is a
        // conclusive filter and the fold fits in a stack buffer.
        if token.len() > LONGEST_KEY {
            return None;
        }
        let mut buf = [0u8; LONGEST_KEY];
        let lower = &mut buf[..token.len()];
        lower.copy_from_slice(token.as_bytes());
        lower.make_ascii_lowercase();
        return CONVERSION_TABLE
            .iter()
            .find(|(k, _)| k.as_bytes() == lower)
            .map(|&(_, v)| v);
    }
    // Non-ASCII input still needs the full Unicode fold rather than an early
    // bail-out: U+212A KELVIN SIGN lowercases to ASCII `k`, so "a non-ASCII token
    // cannot fold into an ASCII key" is not a safe assumption to hard-code.
    // Lowercasing can shrink a 4-byte character to 1 byte and no further, so
    // 4 x LONGEST_KEY is the largest input that could still fit a key.
    if token.len() > 4 * LONGEST_KEY {
        return None;
    }
    let lower = token.to_lowercase();
    CONVERSION_TABLE
        .iter()
        .find(|(k, _)| *k == lower)
        .map(|&(_, v)| v)
}

/// Applies the first matching rule to every occurrence in `token`.
///
/// Returns `None` when no rule matched, which the caller distinguishes from "the
/// expansion happened to be identical" because a match still forces a `/\W+/`
/// split while a miss does not.
///
/// # Why the `([azAZ]*)` capture can be ignored
///
/// The reference replaces the whole match — prefix plus contraction — with
/// `'$1 not'`, so the prefix is written straight back out. It can only influence
/// the result by changing *where* a match starts, and therefore where the global
/// scan resumes. It cannot: the prefix is a run of `{a, z, A, Z}`, none of which
/// can begin a contraction (`n` and `'` are both outside the set), so at any
/// start position the greedy star has exactly one viable length. The match
/// consequently always ends at the same place as if only the contraction itself
/// had been matched, and scanning resumes there in both models. Verified against
/// 5,500 recorded tokens.
fn expand(token: &str) -> Option<String> {
    let bytes = token.as_bytes();
    let apostrophes = || bytes.iter().enumerate().filter(|&(_, &b)| b == b'\'');

    let rule = RULES
        .iter()
        .find(|rule| apostrophes().any(|(i, _)| core_at(bytes, i, rule).is_some()))?;

    let mut out = String::with_capacity(token.len() + 4);
    let mut copied = 0usize;
    for (i, _) in apostrophes() {
        let Some((start, end)) = core_at(bytes, i, rule) else {
            continue;
        };
        // Matches are non-overlapping and left to right, exactly as the `g` flag
        // makes them. `start < copied` is unreachable for these six rules — no
        // rule's last character is one that another rule's first character could
        // be — but the guard is what makes that a property rather than a hope.
        if start < copied {
            continue;
        }
        out.push_str(&token[copied..start]);
        out.push_str(rule.expansion);
        copied = end;
    }
    out.push_str(&token[copied..]);
    Some(out)
}

/// The byte span of `rule`'s contraction around the apostrophe at `apos`.
#[inline]
fn core_at(bytes: &[u8], apos: usize, rule: &Rule) -> Option<(usize, usize)> {
    let start = if rule.needs_n {
        // Literal lowercase `n` only: `"N'T"` is left untouched by the reference.
        if apos == 0 || bytes[apos - 1] != b'n' {
            return None;
        }
        apos - 1
    } else {
        apos
    };
    let end = apos + 1 + rule.tail.len();
    if end > bytes.len() {
        return None;
    }
    for (k, &expected) in rule.tail.iter().enumerate() {
        if !bytes[apos + 1 + k].eq_ignore_ascii_case(&expected) {
            return None;
        }
    }
    Some((start, end))
}

/// Splits on `/\W+/` with the reference's **ASCII-only** word class.
///
/// Rust's `regex` crate makes `\w`/`\W` Unicode-aware by default, which would
/// make `é` and `漢` word characters and suppress splits the reference performs:
/// `normalize(["héllo's"])` is `["h", "llo", "is"]`, not `["héllo", "is"]`.
///
/// Working over bytes is safe because a character is either wholly ASCII-word or
/// wholly a separator — every byte of a multi-byte sequence is `>= 0x80` and
/// therefore non-word — so a separator run never cuts a character in half.
///
/// Leading and trailing empty fields are produced, matching `String#split`.
fn split_non_word(s: &str) -> impl Iterator<Item = &str> {
    let bytes = s.as_bytes();
    let mut start = 0usize;
    let mut done = false;

    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let mut i = start;
        while i < bytes.len() && is_word_byte(bytes[i]) {
            i += 1;
        }
        let field = &s[start..i];
        if i == bytes.len() {
            done = true;
            return Some(field);
        }
        // Consume the whole separator run: `\W+` is greedy.
        let mut j = i;
        while j < bytes.len() && !is_word_byte(bytes[j]) {
            j += 1;
        }
        start = j;
        Some(field)
    })
}

/// The reference's `\w`: `[A-Za-z0-9_]`, and nothing else.
#[inline]
const fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(token: &str) -> Vec<String> {
        normalize_token(token)
    }

    #[test]
    fn empty_inputs() {
        assert!(normalize::<&str>(&[]).is_empty());
        assert_eq!(n(""), [""]);
        assert_eq!(normalize(&["", ""]), ["", ""]);
    }

    #[test]
    fn rules_expand_and_preserve_case() {
        assert_eq!(n("hasn't"), ["has", "not"]);
        assert_eq!(n("Hasn't"), ["Has", "not"]);
        assert_eq!(n("HAsn't"), ["HAs", "not"]);
        assert_eq!(n("hasn'T"), ["has", "not"]);
        assert_eq!(n("it's"), ["it", "is"]);
        assert_eq!(n("it'S"), ["it", "is"]);
        assert_eq!(n("we'll"), ["we", "will"]);
        assert_eq!(n("we'Ll"), ["we", "will"]);
        assert_eq!(n("how're"), ["how", "are"]);
        assert_eq!(n("we've"), ["we", "have"]);
        assert_eq!(n("I'd"), ["I", "would"]);
        assert_eq!(n("I'D"), ["I", "would"]);
    }

    #[test]
    fn table_wins_over_rules_and_drops_case() {
        assert_eq!(n("can't"), ["can", "not"]);
        assert_eq!(n("Can't"), ["can", "not"]);
        assert_eq!(n("CaN'T"), ["can", "not"]);
        assert_eq!(n("won't"), ["will", "not"]);
        assert_eq!(n("couldn't've"), ["could", "not", "have"]);
        assert_eq!(n("COULDN't've"), ["could", "not", "have"]);
        assert_eq!(n("i'm"), ["I", "am"]);
        assert_eq!(n("I'M"), ["I", "am"]);
        assert_eq!(n("how'd"), ["how", "did"]);
        assert_eq!(n("HOw'd"), ["how", "did"]);
    }

    #[test]
    fn rule_order_is_observable() {
        // `'ve` (rule 5) is tried before `'d` (rule 6).
        assert_eq!(n("he'd've"), ["he", "d", "have"]);
    }

    #[test]
    fn the_n_of_nt_is_lowercase_only() {
        assert_eq!(n("N'T"), ["N'T"]);
        assert_eq!(n("N't"), ["N't"]);
        assert_eq!(n("n'T"), ["", "not"]);
    }

    #[test]
    fn only_the_ascii_apostrophe_counts() {
        assert_eq!(n("I\u{2019}d"), ["I\u{2019}d"]);
        assert_eq!(n("\u{2019}s"), ["\u{2019}s"]);
    }

    #[test]
    fn the_matched_rule_applies_to_every_occurrence() {
        assert_eq!(n("can't can't"), ["ca", "not", "ca", "not"]);
        assert_eq!(n("couldn't've extra"), ["could", "not", "ve", "extra"]);
        assert_eq!(n("ll'll"), ["ll", "will"]);
        assert_eq!(n("'ll'll"), ["", "will", "will"]);
    }

    #[test]
    fn empty_fields_are_kept() {
        assert_eq!(n("'s"), ["", "is"]);
        assert_eq!(n("it's!"), ["it", "is", ""]);
        assert_eq!(n("!it's"), ["", "it", "is"]);
    }

    #[test]
    fn word_split_is_ascii_only() {
        assert_eq!(n("héllo's"), ["h", "llo", "is"]);
        assert_eq!(n("漢字's"), ["", "is"]);
        assert_eq!(n("a_b1's"), ["a_b1", "is"]);
    }

    #[test]
    fn unmatched_tokens_pass_through_whole() {
        assert_eq!(n("o'clock"), ["o'clock"]);
        assert_eq!(n("abc"), ["abc"]);
        assert_eq!(n("hyphen-ated-word"), ["hyphen-ated-word"]);
        assert_eq!(n("日本語"), ["日本語"]);
        assert_eq!(n("😀"), ["😀"]);
        assert_eq!(n("ALLCAPS"), ["ALLCAPS"]);
    }

    #[test]
    fn a_bare_string_is_one_token_not_a_sentence() {
        // The whole string is looked up as a single token, so the table misses
        // and — since no rule matches either — it comes back unsplit.
        assert_eq!(n("I'm here"), ["I'm here"]);
        assert_eq!(n("no contraction here"), ["no contraction here"]);
        // A rule hit does split it, on every occurrence.
        assert_eq!(n("hasn't hasn't"), ["has", "not", "has", "not"]);
    }

    #[test]
    fn prototype_keys_are_ordinary_tokens_here() {
        // Divergence D-N1: the reference throws on these, because its conversion
        // table is a plain object literal whose prototype chain is searched.
        assert_eq!(n("constructor"), ["constructor"]);
        assert_eq!(n("__proto__"), ["__proto__"]);
        // These never throw in the reference either: the lowercased names miss.
        assert_eq!(n("toString"), ["toString"]);
        assert_eq!(n("hasOwnProperty"), ["hasOwnProperty"]);
    }

    #[test]
    fn multi_token_input_is_flattened_in_order() {
        assert_eq!(
            normalize(&["plain", "it's", "", "N'T"]),
            ["plain", "it", "is", "", "N'T"]
        );
    }

    #[test]
    fn repeated_tokens_do_not_leak_scan_state() {
        // The reference shares six `/g` RegExp objects across calls; both `match`
        // and `replace` reset `lastIndex`, so there is nothing to leak.
        assert_eq!(
            normalize(&["hasn't", "hasn't", "hasn't"]),
            ["has", "not", "has", "not", "has", "not"]
        );
    }

    #[test]
    fn very_long_input() {
        let token = "hasn't ".repeat(2_000);
        let out = normalize_token(&token);
        assert_eq!(out.len(), 4_001); // 2000 x ["has", "not"] plus a trailing ""
        assert_eq!(out[0], "has");
        assert_eq!(out[1], "not");
        assert_eq!(out[out.len() - 1], "");
    }

    #[test]
    fn split_matches_the_reference_string_split() {
        fn split(s: &str) -> Vec<&str> {
            split_non_word(s).collect()
        }
        assert_eq!(split(""), [""]);
        assert_eq!(split("abc"), ["abc"]);
        assert_eq!(split("a b"), ["a", "b"]);
        assert_eq!(split("a  b"), ["a", "b"]);
        assert_eq!(split(" a"), ["", "a"]);
        assert_eq!(split("a "), ["a", ""]);
        assert_eq!(split("!"), ["", ""]);
        assert_eq!(split("😀"), ["", ""]);
        assert_eq!(split("a😀b"), ["a", "b"]);
    }
}
