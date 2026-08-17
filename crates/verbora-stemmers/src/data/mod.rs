//! Generated rule and word tables.
//!
//! Everything in here was machine-derived by reading the reference tree
//! directly. Nothing is transcribed by hand: the
//! Lancaster rule order, the Carry suffix maps, the thirteen stop-word lists,
//! the 29,932-word Indonesian dictionary, and the per-language gate character
//! classes are all mechanically derived, because every one of them contains at
//! least one detail a human transcriber would have "corrected" — the Spanish
//! character class in the *German* gate, the `/i` flag that widens Cyrillic
//! `а-я` to include U+1C80..U+1C86, the two-leading-space `'  aseis'` entry.
//!
//! DO NOT EDIT BY HAND: these are checked in as data.

pub(crate) mod carry_tables;
pub(crate) mod charsets;
pub(crate) mod gates;
pub(crate) mod indonesian_dict;
pub(crate) mod lancaster_rules;
pub(crate) mod stopwords;
