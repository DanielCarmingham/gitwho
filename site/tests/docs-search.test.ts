import { describe, expect, it } from 'vitest';
import { DOCS } from '../src/docs-map';
import { RECIPES } from '../src/data/recipes';
import {
  buildSearchDocuments,
  normalizeSearchText,
  serializeSearchDocuments,
} from '../src/docs-search';

describe('normalizeSearchText', () => {
  it('turns markdown and TOML source into compact searchable text', () => {
    const text = normalizeSearchText(`# Title

Use \`gitwho doctor\` after setup.

\`\`\`toml
default = "personal"
\`\`\`

[Design](DESIGN.md)
`);

    expect(text).toBe('Title Use gitwho doctor after setup. default = personal Design');
  });
});

describe('buildSearchDocuments', () => {
  it('indexes markdown docs plus the hand-written config and recipes pages', () => {
    const markdown = DOCS.map((doc) => ({
      doc,
      raw: `# ${doc.title}\n\n${doc.title} covers gitwho doctor and credential helper setup.`,
    }));

    const documents = buildSearchDocuments({
      markdown,
      configSource: '# config\n\n[account.personal]\nenv = [{ var = "GH_TOKEN" }]',
      recipes: RECIPES.slice(0, 1),
    });

    expect(documents.map((doc) => doc.href)).toEqual([
      '/docs/install/',
      '/docs/design/',
      '/docs/security/',
      '/docs/config/',
      '/docs/recipes/',
    ]);
    expect(documents.find((doc) => doc.href === '/docs/config/')?.body).toContain('GH_TOKEN');
    expect(documents.find((doc) => doc.href === '/docs/recipes/')?.body).toContain(
      RECIPES[0].title,
    );
  });
});

describe('serializeSearchDocuments', () => {
  it('escapes script-breaking markup while preserving JSON data', () => {
    const json = serializeSearchDocuments([
      { title: 'Config', href: '/docs/config/', body: '</script><p>GH_TOKEN</p>' },
    ]);

    expect(json).not.toContain('</script>');
    expect(JSON.parse(json)[0].body).toBe('</script><p>GH_TOKEN</p>');
  });
});
