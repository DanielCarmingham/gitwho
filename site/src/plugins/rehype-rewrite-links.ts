import { visit } from 'unist-util-visit';
import type { Root, Element } from 'hast';
import { routeFor } from '../docs-map';

const ABSOLUTE = /^([a-z][a-z0-9+.-]*:|\/\/)/i;

/**
 * Rewrite the source markdown's repo-relative links to site routes.
 *
 * Matching is by basename: the same target is written as `DESIGN.md` from
 * INSTALL.md and `docs/DESIGN.md` from the README, and both must land on one
 * route.
 *
 * An unmapped relative link throws. Degrading to a warning here would ship a
 * silent 404 — the precise failure this plugin exists to prevent — so the
 * build fails instead and the map gets updated.
 */
export default function rehypeRewriteLinks() {
  return (tree: Root, file: { path?: string }): void => {
    visit(tree, 'element', (node: Element) => {
      if (node.tagName !== 'a') return;

      const href = node.properties?.href;
      if (typeof href !== 'string') return;
      if (href === '' || href.startsWith('#') || href.startsWith('/')) return;
      if (ABSOLUTE.test(href)) return;

      const [path, fragment] = href.split('#');
      const basename = path.split('/').pop() ?? '';
      const route = routeFor(basename);

      if (route === undefined) {
        throw new Error(
          `Unmapped relative link "${href}" in ${file.path ?? 'unknown file'}.\n` +
            `Add it to ROUTES in src/docs-map.ts, or link to it absolutely.`,
        );
      }

      node.properties.href = fragment ? `${route}#${fragment}` : route;
    });
  };
}
