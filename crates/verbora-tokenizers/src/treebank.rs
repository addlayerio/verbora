//! `TreebankWordTokenizer` — Penn Treebank-style tokenization by rewriting.
//!
//! The reference makes seventeen sequential passes over the text: thirteen
//! contraction rules, a punctuation-padding rule, two comma/quote fixups, a
//! sentence-final-period rule, and finally a whitespace split. Every one of them
//! either **inserts a space** or **deletes whitespace**; none adds, removes or
//! reorders a non-whitespace character.
//!
//! That invariant is what makes this implementation zero-copy. The rewrite is
//! built once in a scratch buffer to get the token boundaries right, and each
//! resulting token is then mapped back onto the byte range of the *original*
//! input it came from, by walking both non-whitespace character sequences in
//! step. Tokens are therefore slices of the caller's string, and the scratch
//! buffer is the only allocation.

use std::sync::LazyLock;

use regex::Regex;

use crate::Tokenize;
use crate::classes;
use crate::utf16::Utf16Token;
use crate::whitespace::{SPACE_CLASS, is_whitespace};

/// The eleven two-part contraction rules, in the reference's order.
///
/// Transcribed regex-for-regex, including the bugs. Rule 12 below is the famous
/// one: `\b(Whad)(dd)(ya)\b` requires the literal `Whadddya`, with three `d`s,
/// so it never fires on real text. "Fixing" it to `Whaddya` would change the
/// output of `tokenize("Whaddya")` from one token to three.
///
/// Every `\b`, `\w` and `\W` is compiled with `(?-u)` because the reference's are
/// ASCII-only, and every `(?i)` with `-u` because the reference's `/i` does not fold
/// `ſ` onto `s` or `K` onto `k` the way Unicode simple folding does.
static CONTRACTIONS_2: LazyLock<[Regex; 11]> = LazyLock::new(|| {
    [
        // `(.)` excludes the four the reference language line terminators, not just `\n`.
        r"([^\n\r\u{2028}\u{2029}])((?i-u:'ll|'re|'ve|n't|'s|'m|'d))(?-u:\b)",
        r"(?i-u)\b(can)(not)\b",
        r"(?i-u)\b(D)('ye)\b",
        r"(?i-u)\b(Gim)(me)\b",
        r"(?i-u)\b(Gon)(na)\b",
        r"(?i-u)\b(Got)(ta)\b",
        r"(?i-u)\b(Lem)(me)\b",
        r"(?i-u)\b(Mor)('n)\b",
        r"(?i-u)\b(T)(is)\b",
        r"(?i-u)\b(T)(was)\b",
        r"(?i-u)\b(Wan)(na)\b",
    ]
    .map(|p| Regex::new(p).expect("static contraction pattern"))
});

/// The two three-part contraction rules.
static CONTRACTIONS_3: LazyLock<[Regex; 2]> = LazyLock::new(|| {
    [r"(?i-u)\b(Whad)(dd)(ya)\b", r"(?i-u)\b(Wha)(t)(cha)\b"]
        .map(|p| Regex::new(p).expect("static contraction pattern"))
});

/// One pattern that matches iff any contraction rule could.
///
/// If it finds nothing in the original text then no rule fires, so no rule can
/// modify the text, so no *later* rule can start firing either — thirteen scans
/// collapse into one. Sound precisely because the rules only ever rewrite what
/// they matched.
static ANY_CONTRACTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"[^\n\r\u{2028}\u{2029}](?i-u:'ll|'re|'ve|n't|'s|'m|'d)(?-u:\b)",
        r"|(?-u:\b)(?i-u:cannot|D'ye|Gimme|Gonna|Gotta|Lemme|Mor'n|Tis|Twas",
        r"|Wanna|Whadddya|Whatcha)(?-u:\b)",
    ))
    .expect("static prescreen pattern")
});

/// `text.replace(/(,\s)/g, ' $1')` — a space before a comma that precedes
/// whitespace.
static COMMA_BEFORE_SPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("(,{SPACE_CLASS})")).expect("static pattern"));

/// `text.replace(/('\s)/g, ' $1')` — the same for apostrophes.
static QUOTE_BEFORE_SPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("('{SPACE_CLASS})")).expect("static pattern"));

/// `text.replace(/\. *(\n|$)/g, ' . ')`.
///
/// `$` has no `m` flag, so it means end-of-string — and the matched newline is
/// **swallowed** by the replacement. The rule therefore fires only at a line end
/// or the very end of the input, which makes tokenization position-dependent: the
/// same sentence yields `"home."` in the middle of a text and `"home", "."` at
/// the end of it.
static FINAL_PERIOD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\. *(?:\n|$)").expect("static pattern"));

/// Penn Treebank-style word tokenizer.
///
/// ```
/// use verbora_tokenizers::{TreebankWordTokenizer, Tokenize};
/// let t = TreebankWordTokenizer::new();
/// assert_eq!(t.tokenize("If we 'all' can't go. I'll stay home."),
///            ["If", "we", "'all", "'", "ca", "n't", "go.", "I", "'ll", "stay", "home", "."]);
/// assert_eq!(t.tokenize("a+b<c>d&e/f-g"), ["a+b<c>d&e/f-g"]);
/// assert_eq!(t.tokenize("e.g. U.S.A."), ["e.g.", "U.S.A", "."]);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreebankWordTokenizer;

impl TreebankWordTokenizer {
    /// Creates the tokenizer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Runs the reference's rewrite pipeline, returning the scratch buffer.
    fn rewrite(text: &str) -> String {
        let mut owned;
        let mut cur = text;
        if ANY_CONTRACTION.is_match(text) {
            owned = text.to_owned();
            for re in CONTRACTIONS_2.iter() {
                owned = re.replace_all(&owned, "${1} ${2}").into_owned();
            }
            for re in CONTRACTIONS_3.iter() {
                owned = re.replace_all(&owned, "${1} ${2} ${3}").into_owned();
            }
            cur = &owned;
        }

        // `text.replace(/([^\w.'\-/+<>,&])/g, ' $1 ')`, done by hand so that an
        // astral character is written once rather than as two padded halves —
        // the token it forms is split back into surrogates at the very end.
        let mut buf = String::with_capacity(cur.len() * 2);
        for c in cur.chars() {
            if classes::is_word_treebank_keep(c) {
                buf.push(c);
            } else {
                buf.push(' ');
                buf.push(c);
                buf.push(' ');
            }
        }

        let buf = COMMA_BEFORE_SPACE.replace_all(&buf, " ${1}").into_owned();
        let buf = QUOTE_BEFORE_SPACE.replace_all(&buf, " ${1}").into_owned();
        FINAL_PERIOD.replace_all(&buf, " . ").into_owned()
    }
}

/// Maps each token of the rewritten buffer back onto the input it came from.
///
/// The rewrite preserves the sequence of non-whitespace characters exactly, so
/// walking the two sequences in lockstep recovers each token's original byte
/// range. Where the recovered range is contiguous — always, in practice, since
/// the only deletions are whitespace and every deletion is followed by an
/// inserted space — the token borrows; the copy path exists so that a future rule
/// change degrades gracefully instead of silently mis-slicing.
fn map_back<'a>(text: &'a str, buf: &str) -> Vec<Utf16Token<'a>> {
    let mut out = Vec::new();
    let mut orig = text.char_indices().filter(|(_, c)| !is_whitespace(*c));

    for tok in buf.split(is_whitespace).filter(|s| !s.is_empty()) {
        let mut start = None;
        let mut end = 0usize;
        let mut contiguous = true;
        for c in tok.chars() {
            let Some((i, oc)) = orig.next() else {
                // Unreachable while the invariant holds; degrade to a copy.
                contiguous = false;
                break;
            };
            debug_assert_eq!(oc, c, "rewrite altered a non-whitespace character");
            match start {
                None => start = Some(i),
                Some(_) if i != end => contiguous = false,
                Some(_) => {}
            }
            end = i + oc.len_utf8();
        }

        // A lone astral character is two tokens in the reference: The reference pads
        // each surrogate half separately, so each becomes its own token.
        let mut chars = tok.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && c.len_utf16() == 2
        {
            let mut units = [0u16; 2];
            c.encode_utf16(&mut units);
            out.push(Utf16Token::from_units(&units[..1]));
            out.push(Utf16Token::from_units(&units[1..]));
            continue;
        }

        match start {
            Some(s) if contiguous => out.push(Utf16Token::borrowed(&text[s..end])),
            _ => out.push(Utf16Token::owned(tok.to_owned())),
        }
    }
    out
}

impl Tokenize for TreebankWordTokenizer {
    type Token<'a> = Utf16Token<'a>;

    /// The pipeline is whole-text, so the work happens eagerly and the iterator
    /// walks the finished list. The signature stays uniform with the streaming
    /// tokenizers so generic code does not have to special-case it.
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = Utf16Token<'a>> {
        let buf = Self::rewrite(text);
        map_back(text, &buf).into_iter()
    }
}

impl verbora_core::Tokenizer for TreebankWordTokenizer {
    /// Tokens containing unpaired surrogates are rendered with U+FFFD here; use
    /// [`Tokenize::tokens`] for the lossless form.
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(Tokenize::tokens(self, text).map(|t| t.to_string_lossy().into_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tb(s: &str) -> Vec<String> {
        TreebankWordTokenizer::new()
            .tokens(s)
            .map(|t| t.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn contractions_split() {
        assert_eq!(tb("don't"), ["do", "n't"]);
        assert_eq!(tb("cannot"), ["can", "not"]);
        assert_eq!(tb("D'ye"), ["D", "'ye"]);
        assert_eq!(tb("Mor'n"), ["Mor", "'n"]);
    }

    #[test]
    fn the_whaddya_rule_never_fires() {
        assert_eq!(tb("Whaddya Whatcha"), ["Whaddya", "Wha", "t", "cha"]);
    }

    #[test]
    fn final_period_rule_is_position_dependent() {
        assert_eq!(tb("a."), ["a", "."]);
        assert_eq!(tb("."), ["."]);
        // The newline is consumed by the replacement.
        assert_eq!(tb("a.\nb."), ["a", ".", "b", "."]);
        // Mid-text the period stays attached; only the last one splits.
        let twice = "I'll stay home. I'll stay home.";
        assert_eq!(
            tb(twice),
            ["I", "'ll", "stay", "home.", "I", "'ll", "stay", "home", "."]
        );
    }

    #[test]
    fn empty_and_whitespace_only() {
        assert_eq!(tb(""), Vec::<String>::new());
        assert_eq!(tb("   "), Vec::<String>::new());
        assert_eq!(tb("\n"), Vec::<String>::new());
    }

    #[test]
    fn ascii_word_characters_only() {
        // The reference's `\w` is ASCII, so accented letters get space-padded.
        assert_eq!(tb("café naïve"), ["caf", "é", "na", "ï", "ve"]);
        assert_eq!(tb("日本語です。"), ["日", "本", "語", "で", "す", "。"]);
    }

    #[test]
    fn punctuation_and_numbers() {
        assert_eq!(tb("3.14"), ["3.14"]);
        assert_eq!(tb("hello, world"), ["hello", ",", "world"]);
        assert_eq!(tb("'quoted'"), ["'quoted'"]);
    }

    #[test]
    fn astral_characters_become_two_tokens() {
        let toks: Vec<Vec<u16>> = TreebankWordTokenizer::new()
            .tokens("test😀emoji")
            .map(|t| t.to_utf16())
            .collect();
        assert_eq!(toks.len(), 4);
        assert_eq!(toks[1], vec![0xd83d]);
        assert_eq!(toks[2], vec![0xde00]);
    }

    #[test]
    fn tokens_borrow_from_the_input() {
        let text = String::from("hello, world");
        let toks: Vec<_> = TreebankWordTokenizer::new().tokens(&text).collect();
        assert!(
            toks.iter()
                .all(|t| matches!(t, Utf16Token::Text(std::borrow::Cow::Borrowed(_))))
        );
    }

    #[test]
    fn very_long_input_scales() {
        let text = "lorem ipsum ".repeat(5_000);
        assert_eq!(TreebankWordTokenizer::new().tokens(&text).count(), 10_000);
    }
}
