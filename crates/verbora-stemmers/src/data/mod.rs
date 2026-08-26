//! The rule and word tables the stemmers read.
//!
//! Four kinds of table live here: the Lancaster rule order, the Carry suffix
//! maps, the Indonesian root dictionary, and the per-language gate character
//! classes. All are checked-in data — nothing here is computed at build time
//! and nothing has a generator in the tree.
//!
//! The thirteen stop-word lists used to live here as well, twice over: once in
//! source order and once as a hand-written sorted table. Both are gone. The
//! data has one home, `verbora-core`, and the sorted view is derived there;
//! `crate::stopwords` keeps only the per-language mutability this crate adds
//! on top of it.
//!
//! **Data is not exempt from review.** These files were previously headed "do
//! not edit by hand", and that is exactly how two stop words stayed dead for
//! the life of the crate: Dutch `"je "` carried a trailing space and German
//! `"ei,"` a trailing comma, so neither could ever equal a token, and no test
//! looked. A table nothing checks is a table nothing knows is right. This
//! module's test-only `audit` submodule walks every entry of every stop-word
//! list through the pipeline that consumes it, and [`gates`]' own test module
//! walks all seven gated lists through the gate that guards them — which is
//! how the German gate was found to be a verbatim copy of the Spanish one,
//! holding `á é í ñ ó ú` and not `ä ö ß`.
//!
//! The remaining three tables have now had the same walk, and it was worth
//! doing. [`lancaster_rules`] is the published Paice/Husk set, in the published
//! order, verified rule by rule against Paice's own distributed file — one of
//! its rules is unreachable, in the publication too. [`carry_tables`] holds two
//! entries that fire where they should not: `yeux -> oeil` is a whole-word rule
//! in a suffix table, and `ien -> i` has no plural partner. [`indonesian_dict`]
//! turned out to be `natural`'s runtime dictionary entry for entry, and 22 of
//! its own hyphenated roots do not stem to themselves.
//!
//! None of that was visible from reading the tables. See the per-file notes.

pub(crate) mod carry_tables;
pub(crate) mod charsets;
pub(crate) mod gates;
pub(crate) mod indonesian_dict;
pub(crate) mod lancaster_rules;

#[cfg(test)]
pub(crate) mod audit;

#[cfg(test)]
mod table_audit;
