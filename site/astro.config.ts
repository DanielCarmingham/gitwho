import { defineConfig } from 'astro/config';
import { unified } from '@astrojs/markdown-remark';
import remarkStripTitle from './src/plugins/remark-strip-title';
import rehypeRewriteLinks from './src/plugins/rehype-rewrite-links';
import rehypeWrapTables from './src/plugins/rehype-wrap-tables';

export default defineConfig({
  // www-canonical, matching every other site in the shared Caddyfile: the apex
  // 301s to www. Declaring the apex here would make every page announce a
  // canonical URL that immediately redirects away. Single source of truth —
  // BaseLayout builds its canonical and og:url from `Astro.site`.
  site: 'https://www.gitwho.cc',
  output: 'static',
  /*
   * Stated rather than left at 'ignore', because the build format already
   * decides it. `build.format` is 'directory', so every page is emitted as
   * `<route>/index.html` and Caddy's file_server 301s `/docs/config` to
   * `/docs/config/`. `Astro.url.pathname` carries the slash either way, so
   * with 'ignore' the canonical URL and every internal link disagreed and each
   * internal navigation cost a redirect. 'always' is the pairing Astro
   * documents for the directory format; every internal href is written with
   * the slash to match.
   */
  trailingSlash: 'always',
  markdown: {
    // Both palettes are emitted as CSS variables and resolved by the site's
    // three theme states in src/styles/global.css. Matches src/components/CodeBlock.astro.
    shikiConfig: {
      themes: { light: 'vitesse-light', dark: 'vitesse-dark' },
      defaultColor: false,
    },
    processor: unified({
      remarkPlugins: [remarkStripTitle],
      rehypePlugins: [rehypeRewriteLinks, rehypeWrapTables],
    }),
  },
});
