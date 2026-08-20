//! The **settled default detector**, and the measured accuracy floor that
//! settles it.
//!
//! # Why this file exists
//!
//! [`HashedLinearDetector`](verbora_language::HashedLinearDetector) is
//! several hundred times faster than
//! [`WhatlangDetector`](verbora_language::WhatlangDetector) and beats
//! `whichlang` — the fastest third-party Rust detector this workspace
//! benchmarks against — at *every* input tier. That is a standing,
//! permanent incentive to promote it to the default, and the only thing
//! that should ever stop someone is a number. This file is that number,
//! executed rather than quoted: it re-scores every shipped detector over
//! the same committed 13-language x 4-tier UDHR corpus that
//! `benchmarks/competitive/rust-competitors/examples/language_accuracy.rs`
//! reports on, and pins each one's per-tier `(correct, abstained)` counts
//! exactly.
//!
//! So a future change that makes the fast model the default does not
//! quietly ship a detector that is **7 items worse over 52** and **less
//! accurate than `whichlang` itself** (45/52 vs. 48/52) while advertising
//! a speed win over it — the "10x faster but substantially less correct,
//! presented as just '10x faster'" outcome this workspace's own benchmark
//! charter forbids. It fails this file first.
//!
//! # The corpus is embedded, not read from disk, on purpose
//!
//! The competitive harness reads
//! `benchmarks/competitive/datasets/language-accuracy/dataset.json`, which
//! lives outside this crate and outside anything a published
//! `verbora-language` would carry. A relative path out of `crates/` would
//! make this test pass or fail on directory layout rather than on detector
//! behaviour, so the corpus is inlined verbatim below (~12 KB, all four
//! tiers, byte-identical to that JSON's `languages` array). See that
//! dataset's own `README.md` for sourcing (OHCHR UDHR Translation Project,
//! public domain), license, and per-tier extraction methodology.
//!
//! Only compiled with `language-detection`; the fast-model comparisons
//! additionally need `fast-language-detection`:
//! `cargo test -p verbora-language --features language-detection,fast-language-detection`.
#![cfg(feature = "language-detection")]

use verbora_language::{DefaultDetector, LanguageDetector, WhatlangDetector};

/// One corpus language at all four tiers — the same shape as one element
/// of the dataset JSON's `languages` array, minus the fields (`udhr_key`,
/// `verbora_language`) only the extraction tooling needs.
struct LanguageItems {
    iso639_1: &'static str,
    short_word: &'static str,
    short_phrase: &'static str,
    sentence: &'static str,
    paragraph: &'static str,
}

impl LanguageItems {
    /// Indexed by tier name so [`TIERS`] can drive the scoring loop, which
    /// keeps every detector scored over the tiers in one fixed order —
    /// the counts pinned below are per-tier, and a mismatched order would
    /// make them silently meaningless rather than loudly wrong.
    fn tier(&self, tier: &str) -> &'static str {
        match tier {
            "short_word" => self.short_word,
            "short_phrase" => self.short_phrase,
            "sentence" => self.sentence,
            "paragraph" => self.paragraph,
            other => panic!("unknown tier {other:?}"),
        }
    }
}

/// Ascending by length, matching the dataset's own tier order.
const TIERS: [&str; 4] = ["short_word", "short_phrase", "sentence", "paragraph"];

/// How one detector scored over one tier. `abstained` is tracked
/// separately from `correct` because the two failure modes are not the
/// same claim: an abstention is this crate's documented honest answer for
/// input with no usable signal, a wrong commit is not — and a detector
/// that traded the first for the second would keep its accuracy count flat
/// while getting materially worse.
#[derive(Debug, PartialEq, Eq)]
struct TierScore {
    correct: usize,
    abstained: usize,
}

/// Scores `detector` over every corpus language at one tier.
fn score(detector: &impl LanguageDetector, tier: &str) -> TierScore {
    let mut correct = 0;
    let mut abstained = 0;
    for item in &CORPUS {
        match detector.detect(item.tier(tier)).best() {
            Some(candidate) => {
                if candidate.language.iso639_1() == item.iso639_1 {
                    correct += 1;
                }
            }
            None => abstained += 1,
        }
    }
    TierScore { correct, abstained }
}

/// Scores `detector` over all four tiers, in [`TIERS`] order.
fn score_all(detector: &impl LanguageDetector) -> [TierScore; 4] {
    TIERS.map(|tier| score(detector, tier))
}

/// The dataset's 13 languages — the overlap `whatlang`, `lingua`, and
/// `whichlang` can all be scored on identically, which is why the
/// competitive harness uses exactly these and why re-scoring them here
/// produces numbers directly comparable to its published table.
const CORPUS: [LanguageItems; 13] = [
    LanguageItems {
        iso639_1: "en",
        short_word: "dignity",
        short_phrase: "All human beings are born",
        sentence: "All human beings are born free and equal in dignity and rights.",
        paragraph: "All human beings are born free and equal in dignity and rights. They are endowed with reason and conscience and should act towards one another in a spirit of brotherhood. Everyone is entitled to all the rights and freedoms set forth in this Declaration, without distinction of any kind, such as race, colour, sex, language, religion, political or other opinion, national or social origin, property, birth or other status. Furthermore, no distinction shall be made on the basis of the political, jurisdictional or international status of the country or territory to which a person belongs, whether it be independent, trust, non‐self‐governing or under any other limitation of sovereignty.",
    },
    LanguageItems {
        iso639_1: "es",
        short_word: "dignidad",
        short_phrase: "Todos los seres humanos nacen",
        sentence: "Todos los seres humanos nacen libres e iguales en dignidad y derechos y, dotados como están de razón y conciencia, deben comportarse fraternalmente los unos con los otros.",
        paragraph: "Todos los seres humanos nacen libres e iguales en dignidad y derechos y, dotados como están de razón y conciencia, deben comportarse fraternalmente los unos con los otros. Toda persona tiene todos los derechos y libertades proclamados en esta Declaración, sin distinción alguna de raza, color, sexo, idioma, religión, opinión política o de cualquier otra índole, origen nacional o social, posición económica, nacimiento o cualquier otra condición. Además, no se hará distinción alguna fundada en la condición política, jurídica o internacional del país o territorio de cuya jurisdicción dependa una persona, tanto si se trata de un país independiente, como de un territorio bajo administración fiduciaria, no autónomo o sometido a cualquier otra limitación de soberanía.",
    },
    LanguageItems {
        iso639_1: "fr",
        short_word: "dignité",
        short_phrase: "Tous les êtres humains naissent",
        sentence: "Tous les êtres humains naissent libres et égaux en dignité et en droits.",
        paragraph: "Tous les êtres humains naissent libres et égaux en dignité et en droits. Ils sont doués de raison et de conscience et doivent agir les uns envers les autres dans un esprit de fraternité. Chacun peut se prévaloir de tous les droits et de toutes les libertés proclamés dans la présente Déclaration, sans distinction aucune, notamment de race, de couleur, de sexe, de langue, de religion, d’opinion politique ou de toute autre opinion, d’origine nationale ou sociale, de fortune, de naissance ou de toute autre situation. De plus, il ne sera fait aucune distinction fondée sur le statut politique, juridique ou international du pays ou du territoire dont une personne est ressortissante, que ce pays ou territoire soit indépendant, sous tutelle, non autonome ou soumis à une limitation quelconque de souveraineté.",
    },
    LanguageItems {
        iso639_1: "de",
        short_word: "Würde",
        short_phrase: "Alle Menschen sind frei und",
        sentence: "Alle Menschen sind frei und gleich an Würde und Rechten geboren.",
        paragraph: "Alle Menschen sind frei und gleich an Würde und Rechten geboren. Sie sind mit Vernunft und Gewissen begabt und sollen einander im Geist der Brüderlichkeit begegnen. Jeder hat Anspruch auf die in dieser Erklärung verkündeten Rechte und Freiheiten ohne irgendeinen Unterschied, etwa nach Rasse, Hautfarbe, Geschlecht, Sprache, Religion, politischer oder sonstiger Überzeugung, nationaler oder sozialer Herkunft, Vermögen, Geburt oder sonstigem Stand. Des Weiteren darf kein Unterschied gemacht werden auf Grund der politischen, rechtlichen oder internationalen Stellung des Landes oder Gebiets, dem eine Person angehört, gleichgültig ob dieses unabhängig ist, unter Treuhandschaft steht, keine Selbstregierung besitzt oder sonst in seiner Souveränität eingeschränkt ist.",
    },
    LanguageItems {
        iso639_1: "it",
        short_word: "dignità",
        short_phrase: "Tutti gli esseri umani nascono",
        sentence: "Tutti gli esseri umani nascono liberi ed eguali in dignità e diritti.",
        paragraph: "Tutti gli esseri umani nascono liberi ed eguali in dignità e diritti. Essi sono dotati di ragione e di coscienza e devono agire gli uni verso gli altri in spirito di fratellanza. Ad ogni individuo spettano tutti i diritti e tutte le libertà enunciate nella presente Dichiarazione, senza distinzione alcuna, per ragioni di razza, di colore, di sesso, di lingua, di religione, di opinione politica o di altro genere, di origine nazionale o sociale, di ricchezza, di nascita o di altra condizione. Nessuna distinzione sarà inoltre stabilita sulla base dello statuto politico, giuridico o internazionale del paese o del territorio cui una persona appartiene, sia indipendente, o sottoposto ad amministrazione fiduciaria o non autonomo, o soggetto a qualsiasi limitazione di sovranità.",
    },
    LanguageItems {
        iso639_1: "pt",
        short_word: "dignidade",
        short_phrase: "Todos os seres humanos nascem",
        sentence: "Todos os seres humanos nascem livres e iguais em dignidade e em direitos.",
        paragraph: "Todos os seres humanos nascem livres e iguais em dignidade e em direitos. Dotados de razão e de consciência, devem agir uns para com os outros em espírito de fraternidade. Todos os seres humanos podem invocar os direitos e as liberdades proclamados na presente Declaração, sem distinção alguma, nomeadamente de raça, de cor, de sexo, de língua, de religião, de opinião política ou outra, de origem nacional ou social, de fortuna, de nascimento ou de qualquer outra situação. Além disso, não será feita nenhuma distinção fundada no estatuto político, jurídico ou internacional do país ou do território da naturalidade da pessoa, seja esse país ou território independente, sob tutela, autónomo ou sujeito a alguma limitação de soberania.",
    },
    LanguageItems {
        iso639_1: "nl",
        short_word: "waardigheid",
        short_phrase: "Alle mensen worden vrij en",
        sentence: "Alle mensen worden vrij en gelijk in waardigheid en rechten geboren.",
        paragraph: "Alle mensen worden vrij en gelijk in waardigheid en rechten geboren. Zij zijn begiftigd met verstand en geweten, en behoren zich jegens elkander in een geest van broederschap te gedragen. Een ieder heeft aanspraak op alle rechten en vrijheden, in deze Verklaring opgesomd, zonder enig onderscheid van welke aard ook, zoals ras, kleur, geslacht, taal, godsdienst, politieke of andere overtuiging, nationale of maatschappelijke afkomst, eigendom, geboorte of andere status. Verder zal geen onderscheid worden gemaakt naar de politieke, juridische of internationale status van het land of gebied, waartoe iemand behoort, onverschillig of het een onafhankelijk, trust-, of niet-zelfbesturend gebied betreft, dan wel of er een andere beperking van de soevereiniteit bestaat.",
    },
    LanguageItems {
        iso639_1: "ru",
        short_word: "достоинстве",
        short_phrase: "Все люди рождаются свободными и",
        sentence: "Все люди рождаются свободными и равными в своем достоинстве и правах.",
        paragraph: "Все люди рождаются свободными и равными в своем достоинстве и правах. Они наделены разумом и совестью и должны поступать в отношении друг друга в духе братства. Каждый человек должен обладать всеми правами и всеми свободами, провозглашенными настоящей Декларацией, без какого бы то ни было различия, как-то в отношении расы, цвета кожи, пола, языка, религии, политических или иных убеждений, национального или социального происхождения, имущественного, сословного или иного положения. Кроме того, не должно проводиться никакого различия на основе политического, правового или международного статуса страны или территории, к которой человек принадлежит, независимо от того, является ли эта территория независимой, подопечной, несамоуправляющейся или как-либо иначе ограниченной в своем суверенитете.",
    },
    LanguageItems {
        iso639_1: "hi",
        short_word: "गौरव",
        short_phrase: "सभी मनुष्यों को गौरव और",
        sentence: "सभी मनुष्यों को गौरव और अधिकारों के मामले में जन्मजात स्वतन्त्रता और समानता प्राप्त है ।",
        paragraph: "सभी मनुष्यों को गौरव और अधिकारों के मामले में जन्मजात स्वतन्त्रता और समानता प्राप्त है । उन्हें बुद्धि और अन्तरात्मा की देन प्राप्त है और परस्पर उन्हें भाईचारे के भाव से बर्ताव करना चाहिए । सभी को इस घोषणा में सन्निहित सभी अधिकारों और आज़ादियों को प्राप्त करने का हक़ है और इस मामले में जाति, वर्ण, लिंग, भाषा, धर्म, राजनीति या अन्य विचार-प्रणाली, किसी देश या समाज विशेष में जन्म, सम्पत्ति या किसी प्रकार की अन्य मर्यादा आदि के कारण भेदभाव का विचार न किया जाएगा । इसके अतिरिक्त, चाहे कोई देश या प्रदेश स्वतन्त्र हो, संरक्षित हो, या स्त्रशासन रहित हो या परिमित प्रभुसत्ता वाला हो, उस देश या प्रदेश की राजनैतिक, क्षेत्रीय या अन्तर्राष्ट्रीय स्थिति के आधार पर वहां के निवासियों के प्रति कोई फ़रक़ न रखा जाएगा ।",
    },
    LanguageItems {
        iso639_1: "vi",
        short_word: "phẩm",
        short_phrase: "Tất cả mọi người sinh",
        sentence: "Tất cả mọi người sinh ra đều được tự do và bình đẳng về nhân phẩm và quyền.",
        paragraph: "Tất cả mọi người sinh ra đều được tự do và bình đẳng về nhân phẩm và quyền. Mọi con người đều được tạo hoá ban cho lý trí và lương tâm và cần phải đối xử với nhau trong tình bằng hữu. Mọi người đều được hưởng tất cả những quyền và tự do nêu trong Bản tuyên ngôn này, không phân biệt chủng tộc, màu da, giới tính, ngôn ngữ, tôn giáo, quan điểm chính trị hay các quan điểm khác, nguồn gốc quốc gia hay xã hội, tài sản, thành phần xuất thân hay địa vị xã hội. Ngoài ra, cũng không có bất cứ sự phân biệt nào về địa vị chính trị, pháp quyền hay quốc tế của quốc gia hay lãnh thổ mà một người xuất thân, cho dù quốc gia hay lãnh thổ đó được độc lập, được đặt dưới chế độ uỷ trị, chưa tự quản hay có chủ quyền hạn chế.",
    },
    LanguageItems {
        iso639_1: "ja",
        short_word: "尊厳",
        short_phrase: "すべての人間は、",
        sentence: "すべての人間は、生まれながらにして自由であり、かつ、尊厳と権利とについて平等である。",
        paragraph: "すべての人間は、生まれながらにして自由であり、かつ、尊厳と権利とについて平等である。人間は、理性と良心とを授けられており、互いに同胞の精神をもって行動しなければならない。 すべて人は、人種、皮膚の色、性、言語、宗教、政治上その他の意見、国民的もしくは社会的出身、財産、門地その他の地位又はこれに類するいかなる自由による差別をも受けることなく、この宣言に掲げるすべての権利と自由とを享有することができる。 さらに、個人の属する国又は地域が独立国であると、信託統治地域であると、非自治地域であると、又は他のなんらかの主権制限の下にあるとを問わず、その国又は地域の政治上、管轄上又は国際上の地位に基ずくいかなる差別もしてはならない。",
    },
    LanguageItems {
        iso639_1: "zh",
        short_word: "尊严",
        short_phrase: "人人生而自由，",
        sentence: "人人生而自由，在尊严和权利上一律平等。",
        paragraph: "人人生而自由，在尊严和权利上一律平等。他们赋有理性和良心，并应以兄弟关系的精神相对待。 人人有资格享有本宣言所载的一切权利和自由，不分种族、肤色、性别、语言、宗教、政治或其他见解、国籍或社会出身、财产、出生或其他身分等任何区别。 并且不得因一人所属的国家或领土的政治的、行政的或者国际的地位之不同而有所区别，无论该领土是独立领土、托管领土、非自治领土或者处于其他任何主权受限制的情况之下。",
    },
    LanguageItems {
        iso639_1: "sv",
        short_word: "värde",
        short_phrase: "Alla människor äro födda fria",
        sentence: "Alla människor äro födda fria och lika i värde och rättigheter.",
        paragraph: "Alla människor äro födda fria och lika i värde och rättigheter. De äro utrustade med förnuft och samvete och böra handla gentemot varandra i en anda av broderskap. Envar är berättigad till alla de fri- och rättigheter, som uttalas i denna förklaring, utan åtskillnad av något slag, såsom ras, hudfärg, kön, språk, religion, politisk eller annan uppfattning, nationellt eller socialt ursprung, egendom, börd eller ställning i övrigt. Ingen åtskillnad må vidare göras på grund av den politiska, juridiska eller internationella ställning, som intages av det land eller område, till vilket en person hör, vare sig detta land eller område är oberoende, står under förvaltarskap, är icke-självstyrande eller är underkastat någon annan begränsning av sin suveränitet.",
    },
];

// ---------------------------------------------------------------------------
// 1. What the default *is*.
// ---------------------------------------------------------------------------

/// [`DefaultDetector`] must behave exactly like
/// [`WhatlangDetector`] on every one of the corpus's 52 items —
/// candidate list and confidence included, not merely the winning
/// language.
///
/// This is the pin the whole file exists for. Re-pointing the alias at a
/// different detector is a one-line edit; this test makes that edit fail
/// loudly instead of silently changing what every unqualified "Verbora
/// language detection" claim in this workspace refers to.
#[test]
fn default_detector_is_the_whatlang_wrapper() {
    let default = DefaultDetector::new();
    let whatlang = WhatlangDetector::new();
    for tier in TIERS {
        for item in &CORPUS {
            let text = item.tier(tier);
            assert_eq!(
                default.detect(text),
                whatlang.detect(text),
                "DefaultDetector diverged from WhatlangDetector at {tier}/{}",
                item.iso639_1
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 2. What the default scores — the floor a replacement must clear.
// ---------------------------------------------------------------------------

/// The default's exact per-tier `(correct, abstained)` counts: 49/52
/// overall, with the single abstention and all three misses at the
/// short-word tier.
///
/// Exact equality rather than `>=` is deliberate. A change that *gains* an
/// item here is still a behaviour change worth reading a diff for — this
/// corpus is 13 items per tier, so one item moves a tier's accuracy by
/// ~7.7 points, and a silently drifting reference number is exactly what
/// makes a published accuracy table stop meaning anything.
#[test]
fn default_detector_accuracy_floor() {
    assert_eq!(
        score_all(&DefaultDetector::new()),
        [
            TierScore {
                correct: 10,
                abstained: 1
            },
            TierScore {
                correct: 13,
                abstained: 0
            },
            TierScore {
                correct: 13,
                abstained: 0
            },
            TierScore {
                correct: 13,
                abstained: 0
            },
        ],
        "the default detector's measured accuracy moved; \
         docs/PERFORMANCE.md, site/benchmarks/competitive.md and this \
         crate's own lib.rs table all quote these counts"
    );
}

// ---------------------------------------------------------------------------
// 3. Why the fast model is not the default.
// ---------------------------------------------------------------------------

/// [`HashedLinearDetector`](verbora_language::HashedLinearDetector)'s own
/// per-tier counts, and — asserted as a relation, not just two frozen
/// tables — the fact that its deficit is concentrated in short input and
/// vanishes entirely at sentence length and beyond.
///
/// The relation is the part that carries the decision. Two independently
/// pinned count tables would both have to be re-derived by hand to notice
/// a regression; `fast <= default` per tier, strict at the two short
/// tiers, states the actual reason the fast model stays opt-in in a form
/// that keeps holding if both tables are legitimately re-measured.
#[cfg(feature = "fast-language-detection")]
#[test]
fn fast_detector_is_materially_less_accurate_on_short_input() {
    use verbora_language::HashedLinearDetector;

    let fast = score_all(&HashedLinearDetector::new());
    let default = score_all(&DefaultDetector::new());

    assert_eq!(
        fast,
        [
            TierScore {
                correct: 7,
                abstained: 3
            },
            TierScore {
                correct: 12,
                abstained: 1
            },
            TierScore {
                correct: 13,
                abstained: 0
            },
            TierScore {
                correct: 13,
                abstained: 0
            },
        ],
        "the fast detector's measured accuracy moved; re-run \
         `cargo run --release --example language_accuracy` in \
         benchmarks/competitive/ and re-settle the default on the new numbers"
    );

    // Short tiers: strictly worse, which is the whole reason this detector
    // is opt-in rather than the default.
    assert!(
        fast[0].correct < default[0].correct,
        "fast detector no longer loses at short_word ({} vs {}) -- if this \
         holds after a real re-measurement, the default is worth re-settling",
        fast[0].correct,
        default[0].correct
    );
    assert!(
        fast[1].correct < default[1].correct,
        "fast detector no longer loses at short_phrase ({} vs {})",
        fast[1].correct,
        default[1].correct
    );
    // Long tiers: no deficit at all -- the trade-off is length-dependent,
    // and a caller who only ever sees sentences pays nothing for the speed.
    assert_eq!(
        fast[2].correct, default[2].correct,
        "sentence tier is where both detectors are perfect; a divergence \
         here changes which detector a sentence-only caller should pick"
    );
    assert_eq!(fast[3].correct, default[3].correct);

    let fast_total: usize = fast.iter().map(|t| t.correct).sum();
    let default_total: usize = default.iter().map(|t| t.correct).sum();
    assert_eq!((fast_total, default_total), (45, 49));
}

/// [`FallbackDetector`](verbora_language::FallbackDetector)`<Hashed,
/// Whatlang>` — the composition that answers the "can we have the speed
/// without the accuracy cost?" question honestly — matches the default
/// **tier for tier**, and abstains nowhere.
///
/// Equal accuracy is not equal *answers*: at `short_word` the two score
/// the same 10 on partly different items (the fast model commits wrongly
/// on inputs the default abstains on, and vice versa — see
/// `src/fallback.rs`'s own table). This test pins the counts, which is
/// what a published accuracy table can claim; it deliberately does not
/// assert answer-for-answer equality, which is false.
#[cfg(feature = "fast-language-detection")]
#[test]
fn fallback_matches_the_default_tier_for_tier() {
    use verbora_language::{FallbackDetector, HashedLinearDetector};

    let fallback = score_all(&FallbackDetector::new(
        HashedLinearDetector::new(),
        WhatlangDetector::new(),
    ));
    let default = score_all(&DefaultDetector::new());

    for (tier, (composed, reference)) in TIERS.iter().zip(fallback.iter().zip(default.iter())) {
        assert_eq!(
            composed.correct, reference.correct,
            "fallback no longer matches the default at {tier}"
        );
        assert_eq!(
            composed.abstained, 0,
            "the composition abstains only where *both* sides do, and on \
             this corpus that is nowhere: the reference's single abstention \
             is a short_word item the fast model answers, so the fallback \
             never reaches it (tier {tier})"
        );
    }
}
