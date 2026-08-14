import { describe, it, expect } from 'vitest';
import { unified } from 'unified';
import rehypeParse from 'rehype-parse';
import rehypeStringify from 'rehype-stringify';
import rehypeRewriteLinks from '../src/plugins/rehype-rewrite-links';

const run = (html: string) =>
  unified()
    .use(rehypeParse, { fragment: true })
    .use(rehypeRewriteLinks)
    .use(rehypeStringify)
    .processSync(html)
    .toString();

describe('rehypeRewriteLinks', () => {
  it('rewrites a path-qualified doc link', () => {
    expect(run('<a href="docs/DESIGN.md">d</a>')).toContain('href="/docs/design/"');
  });

  it('rewrites a bare doc link the same way', () => {
    expect(run('<a href="DESIGN.md">d</a>')).toContain('href="/docs/design/"');
  });

  it('preserves the fragment', () => {
    expect(run('<a href="docs/DESIGN.md#known-limits">k</a>')).toContain(
      'href="/docs/design/#known-limits"',
    );
  });

  it('routes the config example to its own page', () => {
    expect(run('<a href="docs/accounts.toml.example">c</a>')).toContain('href="/docs/config/"');
  });

  it('leaves fragment-only links alone', () => {
    expect(run('<a href="#the-config">c</a>')).toContain('href="#the-config"');
  });

  it('leaves absolute links alone', () => {
    const url = 'https://github.com/DanielCarmingham/gitwho';
    expect(run(`<a href="${url}">g</a>`)).toContain(`href="${url}"`);
  });

  it('throws on a relative link it cannot map', () => {
    expect(() => run('<a href="docs/CONTRIBUTING.md">c</a>')).toThrow(/CONTRIBUTING\.md/);
  });
});
