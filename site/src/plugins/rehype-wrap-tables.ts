import { visit } from 'unist-util-visit';
import type { Element, Root } from 'hast';

/** The class the site's stylesheet gives a horizontal scroll container. */
const WRAPPER_CLASS = 'scroll-x';

/** `className` is a string, an array, or absent, depending on the producer. */
function classList(node: Element): string[] {
  const value = node.properties?.className;
  if (Array.isArray(value)) return value.map(String);
  if (typeof value === 'string') return value.split(/\s+/);
  return [];
}

/**
 * Wrap every markdown `<table>` in a horizontally scrolling container.
 *
 * A wide table must not widen the page, and the obvious fix — `display: block`
 * plus `overflow-x: auto` on the table itself — is a quiet accessibility bug:
 * a browser derives a table's accessible role from its *computed* display, so
 * a `display: block` table exposes as a generic container and every row and
 * column relationship in it disappears. The table has to keep `display: table`
 * and something else has to do the scrolling.
 *
 * Markdown tables arrive with no wrapper element to put that on, so this adds
 * one — the same shape `PlatformTable.astro` writes by hand. `tabindex="0"`
 * comes with it: Chrome and Safari give no keyboard route to the right-hand
 * side of a scroll container that cannot be focused (WCAG 2.1.1). No `role` is
 * set, deliberately — `role="region"` without an accessible name announces
 * nothing and only adds noise.
 */
export default function rehypeWrapTables() {
  return (tree: Root): void => {
    visit(tree, 'element', (node: Element, index, parent) => {
      if (node.tagName !== 'table') return;
      if (parent === undefined || index === undefined) return;

      // Idempotent: a table already inside a wrapper is left alone, so running
      // the plugin twice cannot nest scrollers.
      if (
        parent.type === 'element' &&
        parent.tagName === 'div' &&
        classList(parent).includes(WRAPPER_CLASS)
      ) {
        return;
      }

      const wrapper: Element = {
        type: 'element',
        tagName: 'div',
        properties: { className: [WRAPPER_CLASS], tabIndex: 0 },
        children: [node],
      };

      parent.children[index] = wrapper;
    });
  };
}
