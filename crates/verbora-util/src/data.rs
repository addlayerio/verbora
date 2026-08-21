//! Static data tables: the abbreviation lists.
//!
//! **Data, not derivation.** Nothing in this file is computed; it is checked in
//! as the input to [`crate::abbreviations`], and the tests in that module are
//! what establish that it is well formed. Edits here are edits to Verbora's
//! behaviour and are reviewed as such.
//!
//! Each list appears **once**, in source order, keeping whatever duplicates it
//! has — that slice is what `AbbreviationLanguage::abbreviations` returns, so
//! removing a duplicate would change it. The sorted, de-duplicated copy
//! membership searches is derived from it at first use rather than written out
//! a second time; see `abbreviations::SORTED`.
//!
//! The sixteen stop-word lists used to live here too, and a byte-identical copy
//! of thirteen of them lived in `verbora-stemmers`. Both copies are gone: the
//! data now has one home, `verbora-core`, which is the only crate both of them
//! depend on. `crate::Language` is `verbora_core::StopWordLanguage`.

/// English abbreviations.
///
/// 24 entries, all distinct. Order carries no meaning: the consumer
/// (`SentenceTokenizer`) asks whether *any* entry is a suffix of the text
/// before a boundary, so permuting the list cannot change a decision. See
/// [`AbbreviationLanguage`](crate::AbbreviationLanguage).
pub static ABBREVIATIONS_EN: &[&str] = &[
    "approx.", "appt.", "apt.", "A.S.A.P.", "B.Y.O.B.", "c/o", "dept.", "D.I.Y.", "Dr.", "e.g.",
    "est.", "E.T.A.", "Inc.", "min.", "misc.", "Mr.", "Mrs.", "no.", "R.S.V.P.", "tel.", "temp.",
    "vet.", "vs.", "i.e.",
];

/// Spanish abbreviations.
///
/// 108 entries in four thematic groups — titles, general, legal, legal Latin —
/// of which 107 are distinct; some carry internal spaces (`"et al."`,
/// `"a posteriori."`). Neither the grouping nor the duplicate affects a
/// sentence boundary. See [`AbbreviationLanguage`](crate::AbbreviationLanguage).
pub static ABBREVIATIONS_ES: &[&str] = &[
    "Sr.",
    "Sra.",
    "Srta.",
    "Srs.",
    "Sras.",
    "Dr.",
    "Dra.",
    "Drs.",
    "Dras.",
    "Lic.",
    "Licda.",
    "Licdo.",
    "Licds.",
    "Ings.",
    "Ing.",
    "Arq.",
    "Arqs.",
    "Prof.",
    "Profa.",
    "Profs.",
    "Profas.",
    "etc.",
    "e.g.",
    "i.e.",
    "p.ej.",
    "p.e.",
    "a.m.",
    "p.m.",
    "núm.",
    "núms.",
    "n.os",
    "n.os.",
    "ud.",
    "uds.",
    "c/ap.",
    "c/u.",
    "s/n.",
    "av.",
    "pto.",
    "ptos.",
    "pág.",
    "págs.",
    "vol.",
    "vols.",
    "ed.",
    "eds.",
    "cap.",
    "caps.",
    "mín.",
    "máx.",
    "aprox.",
    "ant.",
    "sig.",
    "hist.",
    "biol.",
    "quím.",
    "mat.",
    "psic.",
    "adj.",
    "adv.",
    "art.",
    "arts.",
    "vb.",
    "vbs.",
    "sust.",
    "susts.",
    "prep.",
    "preps.",
    "Art.",
    "Arts.",
    "Inc.",
    "Incs.",
    "const.",
    "Cód.",
    "Códs.",
    "C.C.",
    "C.P.",
    "C.N.",
    "DNU.",
    "DTO.",
    "Res.",
    "Disp.",
    "Disps.",
    "C.P.C.C.",
    "C.C.Y.C.",
    "expte.",
    "exptes.",
    "fs.",
    "fjs.",
    "op.",
    "cf.",
    "cit.",
    "loc. cit.",
    "ut supra.",
    "vgr.",
    "ap.",
    "cfr.",
    "ss.",
    "et al.",
    "ibid.",
    "ibíd.",
    "op. cit.",
    "loc. cit.",
    "id.",
    "vs.",
    "a priori.",
    "a posteriori.",
    "sine die.",
];
