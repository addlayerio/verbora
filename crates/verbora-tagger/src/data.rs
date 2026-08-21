//! The one data table this crate ships, embedded at build time.
//!
//! `build.rs` reads `data/English/tr_from_brill_paper.json` and emits the rule
//! strings as a `static` array, so nothing is parsed at run time. See
//! `data/NOTICE.md` for the provenance of those rules, and for the record of the
//! lexicons this crate used to bundle and no longer does.

include!(concat!(env!("OUT_DIR"), "/generated.rs"));
