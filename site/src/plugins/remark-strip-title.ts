import type { Root } from 'mdast';

/**
 * Remove the document's leading H1.
 *
 * The gitwho docs carry no frontmatter and open with an H1. The site's layout
 * renders the title itself (from DOCS), so leaving this node in would render
 * the title twice. Only a *leading* H1 is removed — an H1 further down is
 * real content and is left alone.
 */
export default function remarkStripTitle() {
  return (tree: Root): void => {
    const first = tree.children[0];
    if (first && first.type === 'heading' && first.depth === 1) {
      tree.children.shift();
    }
  };
}
