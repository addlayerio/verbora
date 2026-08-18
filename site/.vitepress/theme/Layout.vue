<script setup lang="ts">
import DefaultTheme from 'vitepress/theme';
import { useSidebar } from 'vitepress/theme';
import BuyMeACoffee from './BuyMeACoffee.vue';

const { Layout } = DefaultTheme;

// The support button renders exactly once per page, from one of two slots:
// under the last sidebar entry where there is a sidebar (which is also what
// the mobile drawer renders), and after the content where there is not — today
// only the home page.
//
// `hasSidebar` comes from the sidebar config and the page frontmatter, not from
// viewport width, so it resolves identically on the server and the client and
// cannot cause a hydration mismatch.
const { hasSidebar } = useSidebar();
</script>

<template>
  <Layout>
    <template #sidebar-nav-after>
      <BuyMeACoffee />
    </template>

    <template #doc-footer-before>
      <BuyMeACoffee v-if="!hasSidebar" />
    </template>
  </Layout>
</template>
