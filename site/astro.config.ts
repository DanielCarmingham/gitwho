import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import remarkStripTitle from './src/plugins/remark-strip-title';
import rehypeRewriteLinks from './src/plugins/rehype-rewrite-links';

export default defineConfig({
  site: 'https://gitwho.cc',
  output: 'static',
  markdown: {
    // Both palettes are emitted as CSS variables and resolved by the site's
    // three theme states in src/styles/global.css. Matches src/components/CodeBlock.astro.
    shikiConfig: {
      themes: { light: 'vitesse-light', dark: 'vitesse-dark' },
      defaultColor: false,
    },
    processor: unified({
      remarkPlugins: [remarkStripTitle],
      rehypePlugins: [rehypeRewriteLinks],
    }),
  },
});
