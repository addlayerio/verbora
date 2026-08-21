import { defineConfig } from 'vitepress';

// GitHub Pages redirects the project URL to the configured custom domain.
// The published site therefore lives at the domain root, not at `/verbora/`.
// Keeping this aligned is essential: VitePress prefixes every JS, CSS, font
// and image URL with `base`.
const HOSTNAME = 'https://verbora.dev';
const BASE = '/';

export default defineConfig({
  base: BASE,
  lang: 'en-US',
  title: 'Verbora',
  titleTemplate: ':title · Verbora',
  description:
    'A Rust-native NLP toolkit — what each API does, when to use it, and what it costs.',
  cleanUrls: true,
  lastUpdated: true,
  metaChunk: true,
  sitemap: { hostname: `${HOSTNAME}${BASE}` },

  markdown: {
    // Warm, low-saturation Shiki themes: they sit on the oxide code grounds
    // (theme/oxide.css §7) without fighting the rust accent.
    theme: { light: 'vitesse-light', dark: 'vitesse-dark' },
    lineNumbers: false,

    config(md) {
      // Wrap every Markdown table in a scroll container. The wrapper owns the
      // frame, the rounded corners and the overflow (theme/oxide.css §8), so a
      // wide table scrolls inside its own box instead of widening the page.
      // `renderToken` is used rather than a literal '<table>' so any attributes
      // markdown-it attached to the token survive.
      md.renderer.rules.table_open = (tokens, idx, options, _env, self) =>
        '<div class="table-wrap">' + self.renderToken(tokens, idx, options);
      md.renderer.rules.table_close = (tokens, idx, options, _env, self) =>
        self.renderToken(tokens, idx, options) + '</div>';
    },
  },

  head: [
    // Icons. The SVG covers modern browsers, the .ico (16/32/48) covers the
    // rest, and the platform files below cover iOS, Android, Safari's pinned
    // tabs and Windows tiles. site.webmanifest and browserconfig.xml use
    // manifest-relative paths, so they survive a change of BASE on their own.
    ['link', { rel: 'icon', type: 'image/svg+xml', href: `${BASE}favicon.svg` }],
    ['link', { rel: 'icon', type: 'image/png', sizes: '96x96', href: `${BASE}favicon-96x96.png` }],
    ['link', { rel: 'shortcut icon', href: `${BASE}favicon.ico` }],
    ['link', { rel: 'apple-touch-icon', sizes: '180x180', href: `${BASE}apple-touch-icon.png` }],
    ['link', { rel: 'mask-icon', href: `${BASE}safari-pinned-tab.svg`, color: '#ff8a1e' }],
    ['link', { rel: 'manifest', href: `${BASE}site.webmanifest` }],
    ['meta', { name: 'apple-mobile-web-app-title', content: 'Verbora' }],
    ['meta', { name: 'application-name', content: 'Verbora' }],
    ['meta', { name: 'msapplication-TileColor', content: '#0c0e0f' }],
    ['meta', { name: 'msapplication-config', content: `${BASE}browserconfig.xml` }],
    // Matches the page grounds in theme/oxide.css §2–§3, so the browser chrome
    // continues the page instead of framing it.
    ['meta', { name: 'theme-color', media: '(prefers-color-scheme: light)', content: '#fbfaf8' }],
    ['meta', { name: 'theme-color', media: '(prefers-color-scheme: dark)', content: '#0c0e0f' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'Verbora' }],
    ['meta', { property: 'og:url', content: `${HOSTNAME}${BASE}` }],
    ['meta', { property: 'og:title', content: 'Verbora — a Rust-native NLP toolkit' }],
    [
      'meta',
      {
        property: 'og:description',
        content:
          'Nineteen focused Rust crates for tokenizing, normalizing, stemming, string distance, phonetics, n-grams, TF-IDF, sentiment, classifiers, POS tagging and WordNet. Iterator-first APIs, measured benchmarks.',
      },
    ],
    ['meta', { property: 'og:image', content: `${HOSTNAME}${BASE}og-image.png` }],
    ['meta', { property: 'og:image:width', content: '1200' }],
    ['meta', { property: 'og:image:height', content: '630' }],
    ['meta', { property: 'og:image:alt', content: 'Verbora' }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ['meta', { name: 'twitter:image', content: `${HOSTNAME}${BASE}og-image.png` }],
    [
      'script',
      { type: 'application/ld+json' },
      JSON.stringify({
        '@context': 'https://schema.org',
        '@type': 'SoftwareSourceCode',
        name: 'Verbora',
        programmingLanguage: 'Rust',
        codeRepository: 'https://github.com/addlayerio/verbora',
        description:
          'A high-performance, Rust-native natural language processing toolkit: nineteen focused crates behind one coherent, iterator-first API design.',
        license: 'https://github.com/addlayerio/verbora/blob/main/LICENSE',
      }),
    ],
    // Google Analytics (GA4). `async` so it never blocks first paint, and the
    // inline bootstrap follows it because gtag() queues onto dataLayer before
    // the remote script arrives.
    ['script', { async: '', src: 'https://www.googletagmanager.com/gtag/js?id=G-BK7VXYJHQJ' }],
    [
      'script',
      {},
      `window.dataLayer = window.dataLayer || [];
function gtag(){dataLayer.push(arguments);}
gtag('js', new Date());
gtag('config', 'G-BK7VXYJHQJ');`,
    ],
  ],

  themeConfig: {
    // Vector trace of logo.png (which stays in public/ as the master render):
    // 35 KB instead of 1.4 MB, and sharp at any nav height or pixel ratio.
    logo: '/logo.svg',
    siteTitle: 'Verbora',

    nav: [
      { text: 'Getting started', link: '/getting-started/installation' },
      { text: 'Choosing the right API', link: '/choosing/' },
      { text: 'Features', link: '/features/' },
      { text: 'Performance', link: '/performance/' },
      { text: 'Recipes', link: '/recipes/' },
      {
        text: 'More',
        items: [
          { text: 'Upgrading from 0.1 to 0.2', link: '/getting-started/upgrading' },
          { text: 'Rust API reference', link: '/reference/api' },
          { text: 'Documentation is part of the code', link: '/reference/docs-are-code' },
        ],
      },
    ],

    sidebar: {
      '/getting-started/': sidebarGettingStarted(),
      '/choosing/': sidebarChoosing(),
      '/features/': sidebarFeatures(),
      '/performance/': sidebarPerformance(),
      '/benchmarks/': sidebarPerformance(),
      '/recipes/': sidebarRecipes(),
      '/reference/': sidebarReference(),
    },

    socialLinks: [{ icon: 'github', link: 'https://github.com/addlayerio/verbora' }],

    editLink: {
      pattern: 'https://github.com/addlayerio/verbora/edit/main/site/:path',
      text: 'Edit this page on GitHub',
    },

    search: {
      provider: 'local',
      options: {
        detailedView: true,
      },
    },

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Copyright © Verbora contributors',
    },

    outline: { level: [2, 3] },
  },
});

function sidebarGettingStarted() {
  return [
    {
      text: 'Getting started',
      items: [
        { text: 'Installation', link: '/getting-started/installation' },
        { text: 'Your first program', link: '/getting-started/first-program' },
        { text: 'The workspace map', link: '/getting-started/workspace' },
        { text: 'Cargo features', link: '/getting-started/cargo-features' },
        { text: 'Upgrading from 0.1 to 0.2', link: '/getting-started/upgrading' },
      ],
    },
  ];
}

function sidebarChoosing() {
  return [
    {
      text: 'Choosing the right API',
      items: [
        { text: 'Why there is more than one API', link: '/choosing/' },
        { text: 'The four API shapes', link: '/choosing/api-shapes' },
        { text: 'Tokenization', link: '/choosing/tokenization' },
        { text: 'String distance', link: '/choosing/distance' },
        { text: 'N-grams', link: '/choosing/ngrams' },
        { text: 'Quick answers', link: '/choosing/decision-trees' },
      ],
    },
  ];
}

function sidebarFeatures() {
  return [
    {
      text: 'Features',
      items: [
        { text: 'Overview', link: '/features/' },
        { text: 'Tokenizers', link: '/features/tokenizers' },
        { text: 'String distance', link: '/features/distance' },
        { text: 'Phonetics', link: '/features/phonetics' },
        { text: 'Phonetic neighbors', link: '/features/phonetic-index' },
        { text: 'Beider-Morse', link: '/features/beider-morse' },
        { text: 'Language', link: '/features/language' },
        { text: 'N-grams', link: '/features/ngrams' },
        { text: 'Normalizers', link: '/features/normalizers' },
        { text: 'Inflectors', link: '/features/inflectors' },
        { text: 'Trie', link: '/features/trie' },
        { text: 'Transliterators', link: '/features/transliterators' },
        { text: 'WordNet', link: '/features/wordnet' },
        { text: 'TF-IDF', link: '/features/tfidf' },
        { text: 'Sentiment', link: '/features/sentiment' },
        { text: 'Classifiers', link: '/features/classifiers' },
        { text: 'Stemmers', link: '/features/stemmers' },
        { text: 'Spellcheck', link: '/features/spellcheck' },
        { text: 'POS tagger', link: '/features/tagger' },
        { text: 'Sentence analyzers', link: '/features/analyzers' },
        { text: 'Utilities', link: '/features/util' },
        { text: 'Core vocabulary', link: '/features/core' },
        { text: 'Status and scope', link: '/features/roadmap' },
      ],
    },
  ];
}

function sidebarPerformance() {
  return [
    {
      text: 'How it\'s built',
      items: [
        { text: 'How Verbora uses Rust', link: '/performance/' },
        { text: 'Ergonomics vs throughput', link: '/performance/ergonomics-vs-throughput' },
        { text: 'Iterator vs reusable buffer', link: '/performance/iterator-vs-into' },
        { text: 'Buffer reuse', link: '/performance/buffer-reuse' },
        { text: 'Zero-copy and Cow', link: '/performance/zero-copy' },
        { text: 'Allocation behaviour', link: '/performance/allocation' },
        { text: 'Batch vs streaming', link: '/performance/batch-vs-streaming' },
        { text: 'Parallelism', link: '/performance/parallelism' },
        { text: 'Cache locality and data layout', link: '/performance/cache-locality' },
      ],
    },
    {
      text: 'Results',
      items: [
        { text: 'Method', link: '/benchmarks/' },
        { text: 'String distance results', link: '/benchmarks/distance' },
        { text: 'Competitive benchmarks', link: '/benchmarks/competitive' },
        { text: 'Reproducing them', link: '/benchmarks/reproducing' },
      ],
    },
  ];
}

function sidebarRecipes() {
  return [
    {
      text: 'Recipes',
      items: [
        { text: 'By workload', link: '/recipes/' },
        { text: 'Interactive request/response', link: '/recipes/interactive' },
        { text: 'Streaming', link: '/recipes/streaming' },
        { text: 'Batch corpora', link: '/recipes/batch' },
        { text: 'Massive parallel corpora', link: '/recipes/parallel-corpus' },
        { text: 'Fuzzy name matching', link: '/recipes/fuzzy-matching' },
        { text: 'Prefix autocomplete', link: '/recipes/autocomplete' },
      ],
    },
  ];
}

function sidebarReference() {
  return [
    {
      text: 'Reference',
      items: [
        { text: 'Rust API reference', link: '/reference/api' },
        { text: 'Documentation is part of the code', link: '/reference/docs-are-code' },
      ],
    },
  ];
}
