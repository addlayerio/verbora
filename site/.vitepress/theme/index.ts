import DefaultTheme from 'vitepress/theme';
import type { Theme } from 'vitepress';
import Layout from './Layout.vue';
import HomePaths from './HomePaths.vue';
import './oxide.css';

// The stock VitePress theme, re-skinned with the "oxide" palette in oxide.css.
// The only component is the support button (Layout.vue → BuyMeACoffee.vue),
// which has to be a component because it hangs off theme slots rather than
// page content. Every other visual pattern this site needs (perf cards,
// badges, callouts, decision trees) is plain HTML written directly into the
// Markdown and styled by class name — see oxide.css for the full catalogue.
export default {
  extends: DefaultTheme,
  Layout,
  enhanceApp({ app }) {
    app.component('HomePaths', HomePaths);
  },
} satisfies Theme;
