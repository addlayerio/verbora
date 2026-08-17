import DefaultTheme from 'vitepress/theme';
import type { Theme } from 'vitepress';
import './oxide.css';

// The stock VitePress theme, re-skinned with the "oxide" palette in oxide.css.
// No custom Layout or components: every visual pattern this site needs (perf
// cards, badges, callouts, decision trees) is plain HTML written directly into
// the Markdown and styled by class name — see oxide.css for the full catalogue.
export default {
  extends: DefaultTheme,
} satisfies Theme;
