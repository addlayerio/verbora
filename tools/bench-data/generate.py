#!/usr/bin/env python3
"""Generates the shared benchmark inputs.

Every benchmark harness in the workspace reads these files, so each
implementation is provably measured on byte-identical data. Generating the
inputs from one place -- rather than reimplementing the same generator
alongside each harness -- is what keeps the comparison honest.

Output: benches/data/*.json
"""

import json
import os

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "benches", "data")

MASK64 = (1 << 64) - 1


def lcg(seed):
    """Deterministic 64-bit LCG, so runs are reproducible across machines."""
    state = {"x": seed & MASK64}
    a = 6364136223846793005
    c = 1442695040888963407

    def nxt():
        state["x"] = (state["x"] * a + c) & MASK64
        return state["x"] >> 33

    return nxt


def word(n, seed, alphabet):
    nxt = lcg(seed)
    return "".join(alphabet[nxt() % len(alphabet)] for _ in range(n))


ASCII = list("abcdefghijklmnopqrstuvwxyz")
CYRILLIC = list("\u0430\u0431\u0432\u0433\u0434\u0435\u0436\u0437\u0438\u0439\u043a\u043b\u043c\u043d\u043e\u043f\u0440\u0441\u0442\u0443\u0444\u0445\u0446\u0447\u0448\u0449\u044a\u044b\u044c\u044d\u044e\u044f")

SIZES = [4, 16, 64, 256, 1024]

# Real-word (but not real-sentence) vocabulary for the classification-accuracy
# corpus below: four topical classes, each with 20 signature words a human
# reader would recognise as belonging to that topic, plus a shared pool of
# short, topic-neutral filler words. This is deliberately not a scraped or
# copied real-world corpus (none was available offline for this pass) -- it is
# a synthetic-but-lexically-real stand-in, documented as such everywhere it is
# used. See `docs/PERFORMANCE.md`'s Classifiers section for the caveat this
# carries into the accuracy numbers.
CLASSIFICATION_CLASSES = {
  "sports": [
    "goal",
    "stadium",
    "referee",
    "tournament",
    "coach",
    "athlete",
    "score",
    "league",
    "championship",
    "defense",
    "offense",
    "sprint",
    "medal",
    "teammate",
    "victory",
    "penalty",
    "season",
    "playoff",
    "jersey",
    "crowd"
  ],
  "technology": [
    "software",
    "algorithm",
    "server",
    "database",
    "processor",
    "network",
    "encryption",
    "compiler",
    "interface",
    "bandwidth",
    "firmware",
    "protocol",
    "debugging",
    "framework",
    "latency",
    "cache",
    "kernel",
    "repository",
    "deployment",
    "update"
  ],
  "cooking": [
    "recipe",
    "simmer",
    "skillet",
    "seasoning",
    "garnish",
    "marinade",
    "casserole",
    "pantry",
    "whisk",
    "broth",
    "roast",
    "saute",
    "bakery",
    "ingredient",
    "oven",
    "spatula",
    "grill",
    "dough",
    "glaze",
    "platter"
  ],
  "finance": [
    "portfolio",
    "dividend",
    "equity",
    "ledger",
    "audit",
    "revenue",
    "interest",
    "mortgage",
    "budget",
    "asset",
    "liability",
    "inflation",
    "currency",
    "broker",
    "invoice",
    "expense",
    "capital",
    "insurance",
    "taxation",
    "shareholder"
  ]
}

CLASSIFICATION_NOISE = [
  "the",
  "a",
  "an",
  "and",
  "of",
  "in",
  "on",
  "with",
  "for",
  "was",
  "is",
  "very",
  "quite",
  "really",
  "some",
  "many",
  "their",
  "its",
  "this",
  "that",
  "were",
  "are",
  "has",
  "have",
  "had",
  "but",
  "not",
  "also",
  "more",
  "most"
]

# Fraction of each generated document's words drawn from its class's own
# signature list rather than the shared noise pool -- high enough that a
# working classifier clearly beats chance, low enough that accuracy is not a
# trivial 100% for every implementation (which would make the comparison
# uninformative).
CLASSIFICATION_SIGNAL = 0.55


def classification_doc(rng, class_words, len_min, len_max, signal):
    """One synthetic document: `len_min`..`len_max` words, `signal` fraction
    drawn from `class_words`, the rest from `CLASSIFICATION_NOISE`."""
    n = len_min + (rng() % (len_max - len_min + 1))
    threshold = round(signal * 100)
    out = []
    for _ in range(n):
        if (rng() % 100) < threshold:
            out.append(class_words[rng() % len(class_words)])
        else:
            out.append(CLASSIFICATION_NOISE[rng() % len(CLASSIFICATION_NOISE)])
    return " ".join(out)


# Real, recognizable personal names (given names and surnames) spanning many
# languages and writing systems, romanized/transliterated to plain ASCII
# letters (see `names.json`'s own comment below for why ASCII, not just
# convenience).
#
# Curated by hand, not generated: the phonetic algorithms this feeds
# (SoundEx, Metaphone, Double Metaphone, Daitch-Mokotoff) were designed
# against real name distributions (consonant clusters, silent letters,
# doubled consonants, `-ski`/`-wicz`/`sch`/`th` clusters), and a synthetic
# `word()`-generator draw (uniform random letters, see `words.json` below)
# exercises none of that -- it is realistic *length*, not realistic
# *phonetic content*. This list is a superset of the `SURNAMES` constant
# already hand-picked in `crates/verbora-phonetics/benches/phonetics.rs`,
# extended for breadth across languages.
NAMES = [
  "Smith",
  "Johnson",
  "Williams",
  "Brown",
  "Jones",
  "Miller",
  "Davis",
  "Wilson",
  "Anderson",
  "Taylor",
  "Thomas",
  "Moore",
  "Jackson",
  "Martin",
  "Lee",
  "Thompson",
  "White",
  "Harris",
  "Clark",
  "Lewis",
  "Walker",
  "Young",
  "King",
  "Wright",
  "Green",
  "Baker",
  "Adams",
  "Nelson",
  "Carter",
  "Mitchell",
  "Roberts",
  "Turner",
  "Phillips",
  "Campbell",
  "Parker",
  "Evans",
  "Edwards",
  "Collins",
  "Stewart",
  "Morris",
  "Rogers",
  "Reed",
  "Cook",
  "Morgan",
  "Bell",
  "Murphy",
  "Bailey",
  "Cooper",
  "Richardson",
  "Cox",
  "Howard",
  "Ward",
  "Peterson",
  "Gray",
  "Watson",
  "Brooks",
  "Kelly",
  "Sanders",
  "Price",
  "Bennett",
  "Wood",
  "Barnes",
  "Ross",
  "Henderson",
  "Coleman",
  "Jenkins",
  "Perry",
  "Powell",
  "Long",
  "Patterson",
  "Hughes",
  "Washington",
  "Butler",
  "Simmons",
  "Foster",
  "Bryant",
  "Alexander",
  "Russell",
  "Griffin",
  "Hayes",
  "Knuth",
  "Knight",
  "Xavier",
  "Czech",
  "McDonald",
  "MacDonald",
  "O'Brien",
  "John",
  "Mary",
  "James",
  "Patricia",
  "Robert",
  "Jennifer",
  "Michael",
  "Linda",
  "William",
  "Elizabeth",
  "David",
  "Barbara",
  "Richard",
  "Susan",
  "Joseph",
  "Jessica",
  "Charles",
  "Karen",
  "Christopher",
  "Nancy",
  "Daniel",
  "Margaret",
  "Matthew",
  "Anthony",
  "Garcia",
  "Rodriguez",
  "Martinez",
  "Hernandez",
  "Lopez",
  "Gonzalez",
  "Perez",
  "Sanchez",
  "Ramirez",
  "Torres",
  "Flores",
  "Rivera",
  "Gomez",
  "Diaz",
  "Reyes",
  "Morales",
  "Cruz",
  "Ortiz",
  "Gutierrez",
  "Chavez",
  "Ramos",
  "Vasquez",
  "Castillo",
  "Jimenez",
  "Vargas",
  "Romero",
  "Alvarez",
  "Mendoza",
  "Aguilar",
  "Guzman",
  "Salazar",
  "Delgado",
  "Contreras",
  "Rojas",
  "Navarro",
  "Fuentes",
  "Cabrera",
  "Ibanez",
  "Villarreal",
  "Jose",
  "Juan",
  "Carlos",
  "Miguel",
  "Luis",
  "Antonio",
  "Francisco",
  "Manuel",
  "Pedro",
  "Alejandro",
  "Fernando",
  "Diego",
  "Ricardo",
  "Roberto",
  "Eduardo",
  "Maria",
  "Carmen",
  "Rosa",
  "Isabel",
  "Teresa",
  "Sofia",
  "Lucia",
  "Valentina",
  "Camila",
  "Dubois",
  "Lefevre",
  "Moreau",
  "Simon",
  "Laurent",
  "Lefebvre",
  "Michel",
  "Garnier",
  "Faure",
  "Rousseau",
  "Blanc",
  "Guerin",
  "Muller",
  "Henry",
  "Roussel",
  "Nicolas",
  "Perrin",
  "Morin",
  "Mathieu",
  "Clement",
  "Gauthier",
  "Dumont",
  "Lambert",
  "Bonnet",
  "Francois",
  "Girard",
  "Andre",
  "Mercier",
  "Dupont",
  "Fontaine",
  "Chevalier",
  "Robin",
  "Masson",
  "Jean",
  "Pierre",
  "Philippe",
  "Alain",
  "Bernard",
  "Louis",
  "Isabelle",
  "Nathalie",
  "Sylvie",
  "Catherine",
  "Nicole",
  "Monique",
  "Chantal",
  "Brigitte",
  "Schmidt",
  "Schneider",
  "Fischer",
  "Weber",
  "Meyer",
  "Wagner",
  "Becker",
  "Schulz",
  "Hoffmann",
  "Schafer",
  "Koch",
  "Bauer",
  "Richter",
  "Klein",
  "Wolf",
  "Schroder",
  "Neumann",
  "Schwarz",
  "Zimmermann",
  "Braun",
  "Kruger",
  "Hartmann",
  "Lange",
  "Werner",
  "Krause",
  "Lehmann",
  "Kohler",
  "Herrmann",
  "Konig",
  "Walter",
  "Mayer",
  "Huber",
  "Kaiser",
  "Fuchs",
  "Peters",
  "Scholz",
  "Moller",
  "Weiss",
  "Hans",
  "Klaus",
  "Wolfgang",
  "Gunther",
  "Helmut",
  "Dieter",
  "Rainer",
  "Manfred",
  "Jurgen",
  "Ursula",
  "Helga",
  "Ingrid",
  "Renate",
  "Gisela",
  "Monika",
  "Petra",
  "Sabine",
  "Karin",
  "Erika",
  "Pfeifer",
  "Hochmeier",
  "Schwarzenegger",
  "Rossi",
  "Russo",
  "Ferrari",
  "Esposito",
  "Bianchi",
  "Romano",
  "Colombo",
  "Ricci",
  "Marino",
  "Greco",
  "Bruno",
  "Gallo",
  "Conti",
  "Mancini",
  "Costa",
  "Giordano",
  "Rizzo",
  "Lombardi",
  "Moretti",
  "Barbieri",
  "Fontana",
  "Santoro",
  "Mariani",
  "Rinaldi",
  "Caruso",
  "Ferrara",
  "Galli",
  "Martini",
  "Leone",
  "Giuseppe",
  "Giovanni",
  "Francesco",
  "Marco",
  "Alessandro",
  "Andrea",
  "Luca",
  "Matteo",
  "Lorenzo",
  "Anna",
  "Giulia",
  "Francesca",
  "Chiara",
  "Sara",
  "Laura",
  "Elena",
  "Paola",
  "Silvia",
  "Nowak",
  "Kowalski",
  "Wisniewski",
  "Wojcik",
  "Kowalczyk",
  "Kaminski",
  "Lewandowski",
  "Zielinski",
  "Szymanski",
  "Wozniak",
  "Dabrowski",
  "Kozlowski",
  "Jankowski",
  "Mazur",
  "Kwiatkowski",
  "Krawczyk",
  "Piotrowski",
  "Grabowski",
  "Nowakowski",
  "Pawlowski",
  "Jan",
  "Andrzej",
  "Piotr",
  "Krzysztof",
  "Stanislaw",
  "Tomasz",
  "Pawel",
  "Marek",
  "Grzegorz",
  "Katarzyna",
  "Malgorzata",
  "Agnieszka",
  "Ewa",
  "Elzbieta",
  "Novak",
  "Svoboda",
  "Novotny",
  "Dvorak",
  "Cerny",
  "Prochazka",
  "Kucera",
  "Vesely",
  "Horak",
  "Nemec",
  "Pokorny",
  "Pospisil",
  "Hajek",
  "Jelinek",
  "Kral",
  "Fiala",
  "Hrabal",
  "Ivanov",
  "Smirnov",
  "Kuznetsov",
  "Popov",
  "Sokolov",
  "Lebedev",
  "Kozlov",
  "Novikov",
  "Morozov",
  "Petrov",
  "Volkov",
  "Solovyov",
  "Vasiliev",
  "Zaitsev",
  "Pavlov",
  "Semyonov",
  "Golubev",
  "Vinogradov",
  "Bogdanov",
  "Vorobyov",
  "Fedorov",
  "Mikhailov",
  "Belyaev",
  "Tarasov",
  "Belov",
  "Dmitri",
  "Sergei",
  "Andrei",
  "Nikolai",
  "Ivan",
  "Mikhail",
  "Vladimir",
  "Boris",
  "Igor",
  "Natasha",
  "Olga",
  "Tatiana",
  "Svetlana",
  "Irina",
  "Anastasia",
  "Ekaterina",
  "Shevchenko",
  "Kovalenko",
  "Bondarenko",
  "Tkachenko",
  "Kravchenko",
  "Kovalchuk",
  "Boyko",
  "Melnyk",
  "Shevchuk",
  "Oliynyk",
  "Papadopoulos",
  "Georgiou",
  "Nikolaou",
  "Ioannou",
  "Konstantinou",
  "Dimitriou",
  "Christodoulou",
  "Vasiliou",
  "Antoniou",
  "Stavrou",
  "Ahmed",
  "Mohammed",
  "Hassan",
  "Hussein",
  "Ali",
  "Ibrahim",
  "Khalil",
  "Karim",
  "Youssef",
  "Mahmoud",
  "Farid",
  "Nasser",
  "Rashid",
  "Saleh",
  "Tariq",
  "Aziz",
  "Fatima",
  "Aisha",
  "Layla",
  "Yasmin",
  "Zainab",
  "Amina",
  "Noor",
  "Salma",
  "Cohen",
  "Levi",
  "Mizrahi",
  "Peretz",
  "Biton",
  "Dahan",
  "Avraham",
  "Katz",
  "Friedman",
  "Goldberg",
  "Rosen",
  "Shapiro",
  "Yilmaz",
  "Kaya",
  "Demir",
  "Sahin",
  "Celik",
  "Yildiz",
  "Yildirim",
  "Ozturk",
  "Aydin",
  "Ozdemir",
  "Arslan",
  "Dogan",
  "Kilic",
  "Aslan",
  "Cetin",
  "Wang",
  "Li",
  "Zhang",
  "Liu",
  "Chen",
  "Yang",
  "Huang",
  "Zhao",
  "Wu",
  "Zhou",
  "Xu",
  "Sun",
  "Zhu",
  "Hu",
  "Guo",
  "He",
  "Gao",
  "Lin",
  "Luo",
  "Wei",
  "Fang",
  "Jing",
  "Min",
  "Yan",
  "Jun",
  "Lei",
  "Yong",
  "Hui",
  "Ming",
  "Sato",
  "Suzuki",
  "Takahashi",
  "Tanaka",
  "Watanabe",
  "Ito",
  "Yamamoto",
  "Nakamura",
  "Kobayashi",
  "Kato",
  "Yoshida",
  "Yamada",
  "Sasaki",
  "Yamaguchi",
  "Matsumoto",
  "Inoue",
  "Kimura",
  "Hayashi",
  "Shimizu",
  "Saito",
  "Kim",
  "Park",
  "Choi",
  "Jung",
  "Kang",
  "Cho",
  "Yoon",
  "Jang",
  "Lim",
  "Han",
  "Oh",
  "Seo",
  "Shin",
  "Kwon",
  "Nguyen",
  "Tran",
  "Le",
  "Pham",
  "Hoang",
  "Huynh",
  "Phan",
  "Vu",
  "Vo",
  "Dang",
  "Bui",
  "Do",
  "Ho",
  "Ngo",
  "Duong",
  "Sharma",
  "Verma",
  "Gupta",
  "Singh",
  "Kumar",
  "Patel",
  "Shah",
  "Rao",
  "Reddy",
  "Nair",
  "Iyer",
  "Menon",
  "Chatterjee",
  "Mukherjee",
  "Banerjee",
  "Desai",
  "Joshi",
  "Mehta",
  "Agarwal",
  "Malhotra",
  "Okafor",
  "Okonkwo",
  "Adeyemi",
  "Adebayo",
  "Okoro",
  "Nwosu",
  "Eze",
  "Mensah",
  "Owusu",
  "Kamau",
  "Mwangi",
  "Ochieng",
  "Diallo",
  "Traore",
  "Toure",
  "Santos",
  "Bautista",
  "Villanueva",
  "Andersson",
  "Johansson",
  "Karlsson",
  "Nilsson",
  "Eriksson",
  "Larsson",
  "Olsson",
  "Persson",
  "Svensson",
  "Gustafsson",
  "Hansen",
  "Jensen",
  "Pedersen",
  "Nielsen",
  "Olsen",
  "Andersen",
  "Christensen",
  "Larsen",
  "Sorensen",
  "Rasmussen",
  "de Vries",
  "Jansen",
  "van den Berg",
  "Bakker",
  "Visser",
  "Smit",
  "Meijer",
  "de Boer",
  "Mulder",
  "de Groot",
  "Silva",
  "Oliveira",
  "Souza",
  "Rodrigues",
  "Ferreira",
  "Alves",
  "Pereira",
  "Lima",
  "Gomes",
  "Ribeiro",
  "Martins",
  "Carvalho",
  "Almeida",
  "Barbosa",
  "Nascimento",
  "Araujo"
]

# Realistic per-language word samples for the stemmers, one array per
# stemmer language. Hand-curated real words (not `word()` draws): a
# Snowball/Lancaster/Carry/dictionary stemmer's cost and *output* both depend
# on real morphology (real prefixes, real suffix chains, real vowel/consonant
# runs) that a uniform-random ASCII string does not exercise -- see
# `benchmarks/competitive/rust-competitors/benches/stemmers.rs`'s own doc
# comment, which reads this same file to compare Verbora against
# `rust-stemmers` and `nltk-porter`.
#
# `en` is deliberately real English vocabulary spanning many Porter suffix
# chains (`-ational`, `-iveness`, `-alize`, `-ic`, `-ate`, double consonants,
# `y`-as-vowel, ...) rather than `words.json`'s random strings, because it is
# also the correctness/timing corpus for the English-only `nltk-porter`
# comparison, where realistic suffix coverage is the point.
#
# Russian's `\u0451\u043b\u043a\u0430` is included deliberately, not omitted: it is a real,
# realistic Russian word and this list otherwise mirrors
# `crates/verbora-stemmers/benches/stemmers.rs`'s own `WORDS_RU`. It IS
# excluded -- by filtering, not by omission from this shared list -- from the
# Verbora-vs-`rust-stemmers` comparison in
# `benchmarks/competitive/rust-competitors/benches/stemmers.rs`, because
# Verbora's \u0451->\u0435 fold is not part of the canonical Snowball algorithm
# `rust-stemmers` ports -- see that file's own doc comment and
# `crates/verbora-stemmers/src/ru.rs`.
STEMMER_WORDS = {
  "de": [
    "bedürfnissen",
    "äckern",
    "ackers",
    "armes",
    "derbsten",
    "straße",
    "häuser",
    "fröhlich",
    "quelle",
    "feuer",
    "lichte",
    "bedürfnis",
    "heitkeit",
    "lichen",
    "verstehen",
    "wirtschaft"
  ],
  "es": [
    "árbol",
    "campa",
    "efecto",
    "uyendo",
    "cantándoselo",
    "digámoselo",
    "muéstrame",
    "aseis",
    "hablando",
    "comieron",
    "vivíamos",
    "trabajadores",
    "nacionalidad",
    "rápidamente"
  ],
  "fr": [
    "volera",
    "volerait",
    "subitement",
    "tempérament",
    "voudriez",
    "vengeait",
    "saisissement",
    "transatlantique",
    "premièrement",
    "instruments",
    "trouverions",
    "publicité",
    "pitoyable"
  ],
  "it": [
    "abbandonandoglieli",
    "abbandonarsi",
    "acqua",
    "perché",
    "città",
    "quello",
    "casa",
    "trattamento",
    "nazionale",
    "continuamente",
    "istituzione",
    "abilità"
  ],
  "nl": [
    "aachen",
    "kleurbaar",
    "onaantastbar",
    "lichte",
    "aanbelde",
    "lijkheden",
    "gemeen",
    "maan",
    "brood",
    "jongensgebaren",
    "molenwiekgebaren",
    "verzekeringen"
  ],
  "no": [
    "forebygger",
    "forenkla",
    "havnevirksomhetene",
    "hinder",
    "alltids",
    "akkumulerte",
    "hvorvidt",
    "innovativt",
    "lovleg",
    "arveavgiftslov",
    "vurderingene"
  ],
  "sv": [
    "björks",
    "jaktbössa",
    "klockorna",
    "flickornas",
    "stiftelsen",
    "kloster",
    "frihetens",
    "svenskt",
    "härligt",
    "vackraste",
    "ordentligt",
    "körsbärsträdgårdarna"
  ],
  "pt": [
    "coração",
    "ações",
    "você",
    "abilidades",
    "trabalhadores",
    "nacionalidade",
    "rapidamente",
    "continuamente",
    "instituição",
    "eiras",
    "logias",
    "ativamente"
  ],
  "ru": [
    "важнейшими",
    "важностию",
    "валандался",
    "вагоном",
    "паденье",
    "падчерицей",
    "пакостей",
    "радость",
    "человеческий",
    "государственный",
    "ёлка",
    "мама"
  ],
  "uk": [
    "важливий",
    "милосердість",
    "самостійність",
    "дивовижність",
    "колосальність",
    "миється",
    "читався",
    "радість",
    "гарність",
    "мама",
    "державний"
  ],
  "fa": [
    "کتاب",
    "کتاب‌ها",
    "می‌رود",
    "است",
    "را",
    "در",
    "خانه",
    "بزرگ"
  ],
  "ja": [
    "コーヒー",
    "タクシー",
    "パーティー",
    "コピー",
    "ヘルプ・センター",
    "アイウー",
    "パーティ"
  ],
  "id": [
    "hancurlah",
    "bukumukah",
    "berikanku",
    "dibuang",
    "belajar",
    "pelajar",
    "mempengaruhi",
    "mengkritik",
    "kesepersepuluhnya",
    "meniru-nirukan",
    "buku-buku",
    "perekonomian",
    "memberdayakan",
    "persemakmuran",
    "keberuntunganmu",
    "penstabilan"
  ],
  "en": [
    "running",
    "flies",
    "denied",
    "agreed",
    "plastered",
    "bled",
    "motoring",
    "sizes",
    "hopping",
    "tanned",
    "falling",
    "hissing",
    "fizzed",
    "failing",
    "filing",
    "happy",
    "sky",
    "relational",
    "conditional",
    "rational",
    "valency",
    "hesitancy",
    "digitizer",
    "conformably",
    "radically",
    "differently",
    "vilely",
    "analogously",
    "vietnamization",
    "predication",
    "operator",
    "feudalism",
    "decisiveness",
    "hopefulness",
    "callousness",
    "formality",
    "sensitivity",
    "sensibility",
    "triplicate",
    "formative",
    "formalize",
    "electricity",
    "electrical",
    "hopeful",
    "goodness",
    "revival",
    "allowance",
    "inference",
    "airliner",
    "adjustable",
    "defensible",
    "irritant",
    "replacement",
    "adjustment",
    "dependent",
    "adoption",
    "activate",
    "angularity",
    "effective",
    "national",
    "connection",
    "connections",
    "university",
    "organization"
  ]
}


def write(name, obj):
    path = os.path.join(OUT, name)
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(json.dumps(obj, ensure_ascii=False, separators=(",", ":")))
    size = os.path.getsize(path) / 1024
    print(f"  {name}  ({size:.1f} KiB)")


def main():
    os.makedirs(OUT, exist_ok=True)

    # --- string pairs for the distance metrics ---
    pairs = {"ascii": {}, "cyrillic": {}}
    for n in SIZES:
        pairs["ascii"][str(n)] = [word(n, n * 2 + 1, ASCII), word(n, n * 2 + 2, ASCII)]
    for n in (16, 256):
        pairs["cyrillic"][str(n)] = [word(n, n * 3 + 1, CYRILLIC), word(n, n * 3 + 2, CYRILLIC)]
    write("distance-pairs.json", {"sizes": SIZES, "pairs": pairs})

    # --- word list for batch/throughput work ---
    words = []
    nxt = lcg(99)
    for i in range(20000):
        words.append(word(3 + (nxt() % 12), 1000 + i, ASCII))
    write("words.json", {"words": words})

    # --- realistic multilingual personal names, for the phonetic encoders ---
    # ASCII-letter only, deliberately: rphonetic's `Soundex::encode` (the
    # competitive Rust bench's Soundex competitor) indexes a fixed 26-entry
    # mapping table by `c as usize - 'A' as usize` with no bounds check beyond
    # that arithmetic, so a non-ASCII letter is a real out-of-bounds panic
    # there, not just an unfair comparison -- see
    # `benchmarks/competitive/rust-competitors/benches/phonetics.rs`'s own doc
    # comment. Kept to a curated real-name list (deduplicated) rather than a
    # `word()` random draw: see the `NAMES` comment above for why.
    write("names.json", {"names": list(dict.fromkeys(NAMES))})

    # --- realistic per-language word samples, for the stemmers ---
    write("stemmer-words.json", {"languages": STEMMER_WORDS})

    # --- labeled, signal-bearing corpus for classifier ACCURACY checks -------
    #
    # Unlike every dataset above, this one is not shape-only: a classifier that
    # does nothing but count words can, and should, score above chance on it.
    # It exists specifically for the accuracy dimension
    # `benchmarks/competitive/rust-competitors/benches/classifiers.rs` reads
    # from -- the classifiers' own *speed* benchmarks (Verbora's in-workspace
    # `crates/verbora-classifiers/benches/classifiers.rs`, and this repo's
    # competitive one) intentionally keep using purely-random synthetic tokens
    # instead (see those files' own doc comments for why: cost depends on
    # corpus *shape*, not content, so randomness there does not compromise
    # "same input, same work" -- it just isn't usable for accuracy, which is
    # the gap this dataset fills).
    #
    # `train` is a nested-prefix structure -- `train["16"]` is the first 16
    # documents (4 per class, round-robin) of `train["1024"]` -- so every
    # implementation trained at size N sees a strict superset of what it saw at
    # any smaller N, which is what makes an "accuracy vs. training-set-size"
    # table meaningful rather than five unrelated random draws. `test` is a
    # single fixed 128-document held-out set (32/class), generated from a
    # disjoint part of the RNG stream, shared unchanged across every size.
    classes = list(CLASSIFICATION_CLASSES.keys())
    per_class_max = 1024 // len(classes)
    train_by_class = {}
    for ci, cls in enumerate(classes):
        rng = lcg(0x5EED_0000 + ci)
        train_by_class[cls] = [
            {"text": classification_doc(rng, CLASSIFICATION_CLASSES[cls], 15, 35, CLASSIFICATION_SIGNAL), "label": cls}
            for _ in range(per_class_max)
        ]

    train = {}
    for n in SIZES:
        per_class = n // len(classes)
        docs = []
        for i in range(per_class):
            for cls in classes:
                docs.append(train_by_class[cls][i])
        train[str(n)] = docs

    per_class_test = 32
    test = []
    for ci, cls in enumerate(classes):
        rng = lcg(0x7E57_0000 + ci)  # disjoint seed range from training
        docs = [
            {"text": classification_doc(rng, CLASSIFICATION_CLASSES[cls], 15, 35, CLASSIFICATION_SIGNAL), "label": cls}
            for _ in range(per_class_test)
        ]
        test.extend(docs)

    write("classification-corpus.json", {
        "classes": classes,
        "sizes": SIZES,
        "signal": CLASSIFICATION_SIGNAL,
        "vocab": {"classes": CLASSIFICATION_CLASSES, "noise": CLASSIFICATION_NOISE},
        "train": train,
        "test": test,
    })

    print(f"\nwrote {os.path.normpath(OUT)}")


if __name__ == "__main__":
    main()
