//! The relation symbols that appear in [`Pointer::pointer_symbol`], named.
//!
//! The reference never interprets these — it hands back the raw two-character
//! symbol and leaves the caller to know what `@i` means. The constants and the
//! [`describe`] table below are a `verbora` addition: they add no behaviour,
//! change no output, and exist so that
//! `wordnet.relation(&synset, pointer::HYPERNYM)` reads as what it is.
//!
//! Symbols are drawn from the WordNet 3.1 `wninput(5WN)` manual page. Several
//! are part-of-speech specific: `\` is *derived from adjective* in the adverb
//! file and *pertainym* in the adjective file, and `<` appears only for verbs.
//!
//! [`Pointer::pointer_symbol`]: crate::Pointer::pointer_symbol

/// `!` — antonym. The only lexical (word-to-word) relation in the core set.
pub const ANTONYM: &str = "!";
/// `@` — hypernym: a more general synset ("node" → "computer").
pub const HYPERNYM: &str = "@";
/// `@i` — instance hypernym: the class an individual belongs to.
pub const INSTANCE_HYPERNYM: &str = "@i";
/// `~` — hyponym: a more specific synset.
pub const HYPONYM: &str = "~";
/// `~i` — instance hyponym: a named individual of this class.
pub const INSTANCE_HYPONYM: &str = "~i";
/// `#m` — member holonym: a whole this synset is a member of.
pub const MEMBER_HOLONYM: &str = "#m";
/// `#s` — substance holonym: a whole this synset is a substance of.
pub const SUBSTANCE_HOLONYM: &str = "#s";
/// `#p` — part holonym: a whole this synset is a part of.
pub const PART_HOLONYM: &str = "#p";
/// `%m` — member meronym: a member of this synset.
pub const MEMBER_MERONYM: &str = "%m";
/// `%s` — substance meronym: a substance of this synset.
pub const SUBSTANCE_MERONYM: &str = "%s";
/// `%p` — part meronym: a part of this synset.
pub const PART_MERONYM: &str = "%p";
/// `=` — attribute: the adjective/noun pair "weight" ↔ "heavy".
pub const ATTRIBUTE: &str = "=";
/// `+` — derivationally related form.
pub const DERIVATIONALLY_RELATED: &str = "+";
/// `;c` — domain of synset, topic.
pub const DOMAIN_TOPIC: &str = ";c";
/// `-c` — member of this domain, topic.
pub const MEMBER_TOPIC: &str = "-c";
/// `;r` — domain of synset, region.
pub const DOMAIN_REGION: &str = ";r";
/// `-r` — member of this domain, region.
pub const MEMBER_REGION: &str = "-r";
/// `;u` — domain of synset, usage.
pub const DOMAIN_USAGE: &str = ";u";
/// `-u` — member of this domain, usage.
pub const MEMBER_USAGE: &str = "-u";
/// `*` — entailment (verbs).
pub const ENTAILMENT: &str = "*";
/// `>` — cause (verbs).
pub const CAUSE: &str = ">";
/// `^` — also see.
pub const ALSO_SEE: &str = "^";
/// `$` — verb group.
pub const VERB_GROUP: &str = "$";
/// `&` — similar to (adjective satellites).
pub const SIMILAR_TO: &str = "&";
/// `<` — participle of verb (adjectives).
pub const PARTICIPLE: &str = "<";
/// `\` — pertainym in `data.adj`, derived-from-adjective in `data.adv`.
pub const PERTAINYM: &str = "\\";

/// A human-readable name for a pointer symbol, or `None` if it is unknown.
///
/// `\` is ambiguous by part of speech; the adjective reading is given.
///
/// ```
/// use verbora_wordnet::pointer;
///
/// assert_eq!(pointer::describe("@"), Some("hypernym"));
/// assert_eq!(pointer::describe("%p"), Some("part meronym"));
/// assert_eq!(pointer::describe("??"), None);
/// ```
#[must_use]
pub fn describe(symbol: &str) -> Option<&'static str> {
    // A `match` over string literals compiles to a length-and-bytes decision
    // tree, so this costs no allocation and no hashing.
    Some(match symbol {
        ANTONYM => "antonym",
        HYPERNYM => "hypernym",
        INSTANCE_HYPERNYM => "instance hypernym",
        HYPONYM => "hyponym",
        INSTANCE_HYPONYM => "instance hyponym",
        MEMBER_HOLONYM => "member holonym",
        SUBSTANCE_HOLONYM => "substance holonym",
        PART_HOLONYM => "part holonym",
        MEMBER_MERONYM => "member meronym",
        SUBSTANCE_MERONYM => "substance meronym",
        PART_MERONYM => "part meronym",
        ATTRIBUTE => "attribute",
        DERIVATIONALLY_RELATED => "derivationally related form",
        DOMAIN_TOPIC => "domain of synset - topic",
        MEMBER_TOPIC => "member of this domain - topic",
        DOMAIN_REGION => "domain of synset - region",
        MEMBER_REGION => "member of this domain - region",
        DOMAIN_USAGE => "domain of synset - usage",
        MEMBER_USAGE => "member of this domain - usage",
        ENTAILMENT => "entailment",
        CAUSE => "cause",
        ALSO_SEE => "also see",
        VERB_GROUP => "verb group",
        SIMILAR_TO => "similar to",
        PARTICIPLE => "participle of verb",
        PERTAINYM => "pertainym / derived from adjective",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_constant_is_described() {
        for s in [
            ANTONYM,
            HYPERNYM,
            INSTANCE_HYPERNYM,
            HYPONYM,
            INSTANCE_HYPONYM,
            MEMBER_HOLONYM,
            SUBSTANCE_HOLONYM,
            PART_HOLONYM,
            MEMBER_MERONYM,
            SUBSTANCE_MERONYM,
            PART_MERONYM,
            ATTRIBUTE,
            DERIVATIONALLY_RELATED,
            DOMAIN_TOPIC,
            MEMBER_TOPIC,
            DOMAIN_REGION,
            MEMBER_REGION,
            DOMAIN_USAGE,
            MEMBER_USAGE,
            ENTAILMENT,
            CAUSE,
            ALSO_SEE,
            VERB_GROUP,
            SIMILAR_TO,
            PARTICIPLE,
            PERTAINYM,
        ] {
            assert!(describe(s).is_some(), "{s:?}");
        }
        assert_eq!(describe(""), None);
        assert_eq!(describe("nonsense"), None);
    }
}
