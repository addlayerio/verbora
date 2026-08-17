//! The reference string operations the WordNet reader depends on, each of which
//! has a tempting Rust equivalent that is subtly wrong.
//!
//! | Reference expression | Obvious Rust | Why it diverges |
//! |---|---|---|
//! | `line.split(/\s+/)` | [`str::split_whitespace`] | drops the empty leading/trailing fields the reference counts on |
//! | `word.replace(/\s+/g, '_')` | `replace(char::is_whitespace, "_")` | replaces each *character*, not each *run*; and the `\s` sets differ |
//! | `s.replace(/\s\s+/g, '')` | collapse to one space | the reference **deletes** the run, joining the words |
//! | `key < searchKey` | `&str` `Ord` | UTF-8 byte order, not UTF-16 code-unit order |
//! | `path.join(dir, name)` | [`Path::join`](std::path::Path::join) | Node normalises `.`, `..` and duplicate separators |

use std::borrow::Cow;

use verbora_core::is_whitespace;

/// Whether `b` is one of the ASCII characters in the reference's `\s` class.
///
/// The non-ASCII members (NBSP, the `Zs` block, U+FEFF …) are handled on the
/// slow path; every byte of the shipped WordNet database is ASCII, so this table
/// answers the question for real dictionary lines without decoding anything.
#[inline]
const fn is_ascii_ws(b: u8) -> bool {
    matches!(b, 0x09..=0x0D | 0x20)
}

/// Lazy `String.prototype.split(/\s+/)`.
///
/// Splitting is defined by the *runs* of whitespace, and the pieces between them
/// — including empty ones at either end — are all emitted. That is what makes
/// `find('')` a hit on a WordNet licence-header line: the line starts with two
/// spaces, so `tokens[0]` is `""`.
///
/// ```
/// use verbora_wordnet::whitespace::split_ws;
///
/// let t: Vec<&str> = split_ws("a b").collect();
/// assert_eq!(t, ["a", "b"]);
///
/// // Every WordNet line ends in whitespace, so every token list ends in "".
/// let t: Vec<&str> = split_ws("node n 8  ").collect();
/// assert_eq!(t, ["node", "n", "8", ""]);
///
/// // A licence-header line begins with two spaces.
/// let t: Vec<&str> = split_ws("  20 ABILITY  ").collect();
/// assert_eq!(t, ["", "20", "ABILITY", ""]);
///
/// // And the empty string splits into one empty field, not zero fields.
/// let t: Vec<&str> = split_ws("").collect();
/// assert_eq!(t, [""]);
/// ```
#[inline]
#[must_use]
pub fn split_ws(s: &str) -> SplitWs<'_> {
    SplitWs {
        rest: Some(s),
        ascii: s.is_ascii(),
    }
}

/// Iterator returned by [`split_ws`].
#[derive(Debug, Clone)]
pub struct SplitWs<'a> {
    /// Remaining input, or `None` once the final field has been yielded.
    rest: Option<&'a str>,
    /// Whether the whole input was ASCII, letting the scan stay on bytes.
    ascii: bool,
}

impl<'a> Iterator for SplitWs<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let rest = self.rest?;
        let (run_start, run_end) = if self.ascii {
            let bytes = rest.as_bytes();
            let Some(start) = bytes.iter().position(|&b| is_ascii_ws(b)) else {
                self.rest = None;
                return Some(rest);
            };
            let mut end = start;
            while end < bytes.len() && is_ascii_ws(bytes[end]) {
                end += 1;
            }
            (start, end)
        } else {
            let Some((start, _)) = rest.char_indices().find(|&(_, c)| is_whitespace(c)) else {
                self.rest = None;
                return Some(rest);
            };
            // The run reaches the end of the input unless a non-whitespace
            // character stops it.
            let mut end = rest.len();
            for (i, c) in rest[start..].char_indices() {
                if !is_whitespace(c) {
                    end = start + i;
                    break;
                }
            }
            (start, end)
        };

        let field = &rest[..run_start];
        self.rest = Some(&rest[run_end..]);
        Some(field)
    }
}

impl std::iter::FusedIterator for SplitWs<'_> {}

/// `word.toLowerCase().replace(/\s+/g, '_')` — the normalisation `WordNet#lookup`
/// applies before touching the index.
///
/// Full Unicode lowercasing is required, not [`str::to_ascii_lowercase`]:
/// `'İSTANBUL'` lowercases to nine code units from eight, and `'ΟΔΟΣ'` produces a
/// **final** sigma. Whitespace runs collapse to a *single* underscore, so
/// `"  entity  "` becomes `"_entity_"` — which is why looking a padded word up
/// misses.
///
/// ```
/// use verbora_wordnet::whitespace::normalize_lookup_word;
///
/// assert_eq!(normalize_lookup_word("New York"), "new_york");
/// assert_eq!(normalize_lookup_word("new  york"), "new_york");
/// assert_eq!(normalize_lookup_word("  entity  "), "_entity_");
/// assert_eq!(normalize_lookup_word("ΟΔΟΣ"), "οδος");
/// ```
#[must_use]
pub fn normalize_lookup_word(word: &str) -> String {
    let lowered = word.to_lowercase();
    match replace_ws_runs(&lowered, "_") {
        Cow::Borrowed(_) => lowered,
        Cow::Owned(s) => s,
    }
}

/// Replaces every run of the reference whitespace with `with`, borrowing when there
/// is no whitespace at all (the common case for a single word).
#[must_use]
pub fn replace_ws_runs<'a>(s: &'a str, with: &str) -> Cow<'a, str> {
    if !s.chars().any(is_whitespace) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if is_whitespace(c) {
            if !in_run {
                out.push_str(with);
                in_run = true;
            }
        } else {
            in_run = false;
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// `example.replace(/"/g, '').replace(/\s\s+/g, '')` — the reference's cleanup
/// for the example sentences it splits out of a gloss.
///
/// Two details a "tidy up the whitespace" port gets wrong:
///
/// * runs of two or more whitespace characters are **deleted**, not collapsed, so
///   the words on either side end up joined;
/// * a *single* leading space survives, because one space is not a run.
///
/// Order matters too: quotes are removed first, which can create a whitespace
/// run that then disappears.
///
/// ```
/// use verbora_wordnet::whitespace::clean_example;
///
/// assert_eq!(clean_example(r#""She sighed sadly"  "#), "She sighed sadly");
/// assert_eq!(clean_example(" x  y    z  "), " xyz");
/// ```
#[must_use]
pub fn clean_example(s: &str) -> Cow<'_, str> {
    let has_quote = s.as_bytes().contains(&b'"');
    // A run means two adjacent whitespace characters anywhere.
    let has_run = {
        let mut prev_ws = false;
        let mut found = false;
        for c in s.chars() {
            let ws = is_whitespace(c);
            if ws && prev_ws {
                found = true;
                break;
            }
            prev_ws = ws;
        }
        found
    };
    if !has_quote && !has_run {
        return Cow::Borrowed(s);
    }

    // Pass 1: drop quotes. Pass 2: drop whitespace runs of length >= 2. They
    // cannot be fused, because removing a quote is what creates some of the runs.
    let unquoted: Cow<'_, str> = if has_quote {
        Cow::Owned(s.chars().filter(|&c| c != '"').collect())
    } else {
        Cow::Borrowed(s)
    };

    let mut out = String::with_capacity(unquoted.len());
    let mut run = String::new();
    for c in unquoted.chars() {
        if is_whitespace(c) {
            run.push(c);
        } else {
            if run.chars().count() == 1 {
                out.push_str(&run);
            }
            run.clear();
            out.push(c);
        }
    }
    if run.chars().count() == 1 {
        out.push_str(&run);
    }
    Cow::Owned(out)
}

/// The reference's `<` on two strings: lexicographic over **UTF-16 code units**.
///
/// This decides which way the index bisection turns. Rust's own `&str` ordering
/// compares UTF-8 bytes, and the two disagree for supplementary-plane
/// characters: `'\u{10000}'` encodes as the surrogates `D800 DC00`, which sort
/// *below* `U+E000`, while its UTF-8 bytes sort *above*.
///
/// ```
/// use verbora_wordnet::whitespace::value_lt;
///
/// assert!(value_lt("\u{10000}", "\u{E000}"));      // what the reference says
/// assert!("\u{E000}" < "\u{10000}");            // what Rust's Ord says
/// ```
#[must_use]
pub fn value_lt(a: &str, b: &str) -> bool {
    // ASCII (and in fact anything below U+E000 on both sides) orders identically
    // in UTF-8 and UTF-16, so the common case never allocates or decodes.
    if a.is_ascii() && b.is_ascii() {
        return a.as_bytes() < b.as_bytes();
    }
    a.encode_utf16().lt(b.encode_utf16())
}

/// Node's `path.join(dir, name)` for POSIX paths.
///
/// `IndexFile`'s `filePath` is observable, and Node normalises the result while
/// [`Path::join`](std::path::Path::join) does not: `path.join('./test_data/', 'x')` is
/// `'test_data/x'`, whereas `PathBuf` keeps the `./` and the doubled separator.
///
/// ```
/// use verbora_wordnet::whitespace::path_join;
///
/// assert_eq!(path_join("./test_data/", "index.noun"), "test_data/index.noun");
/// assert_eq!(path_join("a/b/../c", "d"), "a/c/d");
/// assert_eq!(path_join("", "d"), "d");
/// ```
#[must_use]
pub fn path_join(dir: &str, name: &str) -> String {
    // path.join ignores empty segments entirely, then normalises the join.
    let joined = match (dir.is_empty(), name.is_empty()) {
        (true, true) => return ".".to_owned(),
        (true, false) => name.to_owned(),
        (false, true) => dir.to_owned(),
        (false, false) => format!("{dir}/{name}"),
    };
    normalize_posix(&joined)
}

/// `path.normalize` for POSIX: resolves `.`, `..` and duplicate separators.
fn normalize_posix(p: &str) -> String {
    let absolute = p.starts_with('/');
    let trailing = p.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                // Node keeps leading `..` on a relative path and drops it on an
                // absolute one, where there is nothing above the root.
                if let Some(&last) = parts.last() {
                    if last == ".." {
                        parts.push("..");
                    } else {
                        parts.pop();
                    }
                } else if !absolute {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let mut out = parts.join("/");
    if out.is_empty() {
        return if absolute {
            "/".to_owned()
        } else {
            ".".to_owned()
        };
    }
    if trailing {
        out.push('/');
    }
    if absolute { format!("/{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<&str> {
        split_ws(s).collect()
    }

    #[test]
    fn split_keeps_the_empty_edge_fields() {
        assert_eq!(toks(""), [""]);
        assert_eq!(toks(" "), ["", ""]);
        assert_eq!(toks("   "), ["", ""]);
        assert_eq!(toks("a"), ["a"]);
        assert_eq!(toks(" a"), ["", "a"]);
        assert_eq!(toks("a "), ["a", ""]);
        assert_eq!(toks("  a b  "), ["", "a", "b", ""]);
        assert_eq!(toks("a\t\nb"), ["a", "b"]);
        // What `split_whitespace` would have given, for contrast.
        assert_eq!("  a b  ".split_whitespace().collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn split_handles_a_real_index_line() {
        let line = "node n 8 5 ! @ ~ #p ; 8 0 13934060 13918679 13174985 08515608 08515452 05437672 05272412 03832647  ";
        let t = toks(line);
        assert_eq!(t.len(), 20);
        assert_eq!(t[0], "node");
        assert_eq!(t[3], "5");
        assert_eq!(t[19], "");
    }

    #[test]
    fn split_uses_the_reference_whitespace_set() {
        // U+FEFF is whitespace to the reference but not to Rust's char::is_whitespace.
        assert_eq!(toks("a\u{FEFF}b"), ["a", "b"]);
        // U+0085 is the mirror image: Rust says yes, the reference says no.
        assert_eq!(toks("a\u{0085}b"), ["a\u{0085}b"]);
        assert_eq!(toks("a\u{00A0}\u{3000}b"), ["a", "b"]);
    }

    #[test]
    fn split_covers_the_exotic_alphabets() {
        assert_eq!(toks("Москва Ελλάδα"), ["Москва", "Ελλάδα"]);
        assert_eq!(toks("日本語 中文"), ["日本語", "中文"]);
        assert_eq!(toks("😀 a😀b"), ["😀", "a😀b"]);
        assert_eq!(toks("é ﬁ"), ["é", "ﬁ"]);
        assert_eq!(toks(" 😀 "), ["", "😀", ""]);
        assert_eq!(toks("!\"#$ %&'"), ["!\"#$", "%&'"]);
        assert_eq!(toks("123 456"), ["123", "456"]);
        let long = format!("{} {}", "a".repeat(5000), "b".repeat(5000));
        assert_eq!(toks(&long).len(), 2);
    }

    #[test]
    fn normalisation_collapses_runs_to_one_underscore() {
        assert_eq!(normalize_lookup_word(""), "");
        assert_eq!(normalize_lookup_word("a"), "a");
        assert_eq!(normalize_lookup_word("A"), "a");
        assert_eq!(normalize_lookup_word("ENTITY"), "entity");
        assert_eq!(normalize_lookup_word("New York"), "new_york");
        assert_eq!(normalize_lookup_word("new  york"), "new_york");
        assert_eq!(normalize_lookup_word("NEW\tYORK"), "new_york");
        assert_eq!(normalize_lookup_word("  entity  "), "_entity_");
        assert_eq!(normalize_lookup_word("\t\n\r"), "_");
        assert_eq!(normalize_lookup_word("CAFÉ"), "café");
        assert_eq!(normalize_lookup_word("МОСКВА"), "москва");
        assert_eq!(normalize_lookup_word("日本語"), "日本語");
        assert_eq!(normalize_lookup_word("😀"), "😀");
        // Special casing that `to_ascii_lowercase` would not perform.
        assert_eq!(normalize_lookup_word("İSTANBUL"), "i\u{307}stanbul");
    }

    #[test]
    fn example_cleanup_deletes_runs_rather_than_collapsing_them() {
        assert_eq!(clean_example(""), "");
        assert_eq!(clean_example("plain"), "plain");
        assert_eq!(clean_example("\"quoted\""), "quoted");
        assert_eq!(clean_example("a  b"), "ab");
        assert_eq!(clean_example("a b"), "a b");
        assert_eq!(clean_example(" x  y    z  "), " xyz");
        // Quote removal first, whitespace-run removal second.
        assert_eq!(clean_example("a \" b"), "ab");
        assert_eq!(clean_example("日本  語"), "日本語");
    }

    #[test]
    fn string_comparison_is_by_utf16_code_unit() {
        assert!(value_lt("a", "b"));
        assert!(!value_lt("b", "a"));
        assert!(!value_lt("a", "a"));
        assert!(value_lt("", "a"));
        assert!(value_lt("ab", "b"));
        // The case Rust's own Ord gets backwards.
        assert!(value_lt("\u{10000}", "\u{E000}"));
        assert!(!value_lt("\u{E000}", "\u{10000}"));
        assert!("\u{E000}" < "\u{10000}");
        // Below U+E000 the two orders agree.
        assert_eq!(value_lt("é", "z"), "é" < "z");
    }

    #[test]
    fn path_join_normalises_like_node() {
        assert_eq!(
            path_join("./test_data/", "index.document1.txt"),
            "test_data/index.document1.txt"
        );
        assert_eq!(path_join("test_data", "index.noun"), "test_data/index.noun");
        assert_eq!(path_join("/abs/dir", "data.noun"), "/abs/dir/data.noun");
        assert_eq!(path_join("a/./b", "c"), "a/b/c");
        assert_eq!(path_join("a/b/../c", "d"), "a/c/d");
        assert_eq!(path_join("", "d"), "d");
        assert_eq!(path_join(".", "d"), "d");
        assert_eq!(path_join("/", "d"), "/d");
        assert_eq!(path_join("a//b", "c"), "a/b/c");
    }
}
