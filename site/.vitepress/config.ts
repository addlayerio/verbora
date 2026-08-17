import { defineConfig } from 'vitepress';

// Served from GitHub Pages at /verbora/ (project page, not a custom domain).
// If a custom domain is later pointed at this site, flip BASE to '/' and add
// a public/CNAME the way apps/website does for kravn.ai.
const HOSTNAME = 'https://addlayerio.github.io';
const BASE = '/verbora/';

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
    // The "oxide" code rule (theme/custom.css) reads correctly against either
    // Shiki theme; both are kept so the block still highlights under a forced
    // light override.
    theme: { light: 'github-light', dark: 'github-dark' },
    lineNumbers: false,
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
    ['meta', { name: 'msapplication-TileColor', content: '#0d0f10' }],
    ['meta', { name: 'msapplication-config', content: `${BASE}browserconfig.xml` }],
    ['meta', { name: 'theme-color', content: '#0d0f10' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'Verbora' }],
    ['meta', { property: 'og:url', content: `${HOSTNAME}${BASE}` }],
    ['meta', { property: 'og:title', content: 'Verbora — a Rust-native NLP toolkit' }],
    [
      'meta',
      {
        property: 'og:description',
        content:
          'A Rust-native NLP toolkit: 105 APIs across seventeen crates, backed by a 526,341-case regression suite. Choosing the Right API is a first-class section.',
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
          'A high-performance, Rust-native natural language processing toolkit, backed by a 526,341-case regression suite.',
        license: 'https://github.com/addlayerio/verbora/blob/main/LICENSE',
      }),
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
        { text: 'Every decision tree', link: '/choosing/decision-trees' },
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
        { text: 'Core vocabulary', link: '/features/core' },
        { text: 'Roadmap', link: '/features/roadmap' },
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
