//! [`Language`] and [`LanguageSet`]: which of a [`NameType`](crate::NameType)'s
//! own languages a candidate phoneme is still valid under.
//!
//! Every rule's phonetic output can carry alternatives tagged to a specific
//! language (`"in[russian]"`) or left untagged, meaning "valid under every
//! language this name type has" — untagged is not the same thing as the
//! rule-*file* selector `"any"` (which picks which `.txt` file to load when
//! no single language has been pinned down yet); see this module's own
//! `all()` for the untagged case and `beider_morse/mod.rs`'s own doc comment
//! for the file-selector meaning of "any".
//!
//! Each [`NameType`](crate::NameType) has its own, independent language
//! list (Generic: 18, Ashkenazi: 10, Sephardic: 5 — see each name type's own
//! `*_languages.txt`), so a [`Language`]/[`LanguageSet`] value is only
//! meaningful paired with the [`NameType`](crate::NameType) it was produced
//! for — mixing them across name types is a caller error this module does
//! not attempt to prevent at the type level (mirroring
//! [`crate::PhoneticEncoder`]'s own "caller provides consistent input"
//! contract elsewhere in this crate).

/// One language a [`NameType`](crate::NameType)'s rule corpus knows about,
/// as an index into that name type's own language table (0..18 for
/// Generic, 0..10 for Ashkenazi, 0..5 for Sephardic) — never the pseudo-
/// language `"any"` itself, which is a rule-file selector, not a real
/// language a phoneme can be tagged with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Language(pub(crate) u8);

impl Language {
    /// The raw table index. Only meaningful relative to the
    /// [`NameType`](crate::NameType) it was resolved against.
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0
    }
}

/// A set of up to 19 languages, as a bitset — comfortably inside a `u32`,
/// `Copy`, no allocation, cheap to intersect. Every candidate phoneme the
/// encoder builds carries one of these while it is being built, meaning
/// "still valid under every language in this set."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LanguageSet(pub(crate) u32);

impl LanguageSet {
    /// The empty set — no language left. A candidate phoneme reaching this
    /// state is dropped: applying a rule cross-products the running
    /// candidates against that rule's phonetic alternatives and keeps only
    /// the combinations whose two language sets still [`intersect`] to
    /// something, so a join that narrows all the way to this value never
    /// reaches
    /// [`BeiderMorseCode::spellings`](crate::BeiderMorseCode::spellings).
    ///
    /// [`intersect`]: Self::intersect
    pub const EMPTY: Self = Self(0);

    /// A set containing only `language`.
    #[must_use]
    pub const fn single(language: Language) -> Self {
        Self(1 << language.0)
    }

    /// The full set of `count` languages (indices `0..count`) — the
    /// "untagged" meaning: valid under every language this name type has.
    /// `count` is always small (≤ 19), so this never overflows the `u32`.
    #[must_use]
    pub const fn all(count: u8) -> Self {
        if count >= 32 {
            Self(u32::MAX)
        } else {
            Self((1u32 << count) - 1)
        }
    }

    /// Whether this set has no languages left.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The intersection of `self` and `other` — "still valid under both."
    #[must_use]
    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// The union of `self` and `other`.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `self` with every language in `other` removed — used by language
    /// auto-detection's "reject" rules (see `beider_morse/lang.rs`), which
    /// narrow the guess by ruling languages *out* rather than intersecting
    /// them in.
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Whether `language` is a member.
    #[must_use]
    pub const fn contains(self, language: Language) -> bool {
        self.0 & (1 << language.0) != 0
    }

    /// The single language in this set, if it contains exactly one.
    #[must_use]
    pub const fn as_singleton(self) -> Option<Language> {
        if self.0 != 0 && self.0 & (self.0 - 1) == 0 {
            Some(Language(self.0.trailing_zeros() as u8))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_contains_every_index_up_to_count() {
        let s = LanguageSet::all(5);
        for i in 0..5 {
            assert!(s.contains(Language(i)));
        }
        assert!(!s.contains(Language(5)));
    }

    #[test]
    fn intersect_of_disjoint_singletons_is_empty() {
        let a = LanguageSet::single(Language(0));
        let b = LanguageSet::single(Language(1));
        assert!(a.intersect(b).is_empty());
    }

    #[test]
    fn intersect_with_all_is_identity() {
        let all = LanguageSet::all(19);
        let one = LanguageSet::single(Language(7));
        assert_eq!(all.intersect(one), one);
    }

    #[test]
    fn as_singleton_only_true_for_exactly_one_bit() {
        assert_eq!(
            LanguageSet::single(Language(3)).as_singleton(),
            Some(Language(3))
        );
        assert_eq!(LanguageSet::EMPTY.as_singleton(), None);
        assert_eq!(
            LanguageSet::single(Language(1))
                .union(LanguageSet::single(Language(2)))
                .as_singleton(),
            None
        );
    }

    #[test]
    fn union_combines_membership() {
        let a = LanguageSet::single(Language(0));
        let b = LanguageSet::single(Language(1));
        let u = a.union(b);
        assert!(u.contains(Language(0)));
        assert!(u.contains(Language(1)));
        assert!(!u.contains(Language(2)));
    }
}
