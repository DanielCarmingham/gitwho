import { describe, it, expect } from 'vitest';
import { unified } from 'unified';
import rehypeParse from 'rehype-parse';
import rehypeStringify from 'rehype-stringify';
import rehypeWrapTables from '../src/plugins/rehype-wrap-tables';

const run = (html: string) =>
  unified()
    .use(rehypeParse, { fragment: true })
    .use(rehypeWrapTables)
    .use(rehypeStringify)
    .processSync(html)
    .toString();

describe('rehypeWrapTables', () => {
  it('wraps a table in a scroll container', () => {
    expect(run('<table><tr><td>a</td></tr></table>')).toBe(
      '<div class="scroll-x" tabindex="0"><table><tbody><tr><td>a</td></tr></tbody></table></div>',
    );
  });

  it('makes the container keyboard-focusable', () => {
    expect(run('<table><tr><td>a</td></tr></table>')).toContain('tabindex="0"');
  });

  it('leaves the table element itself intact, so its role survives', () => {
    const out = run('<table><thead><tr><th>h</th></tr></thead></table>');
    expect(out).toContain('<table>');
    expect(out).toContain('<th>h</th>');
  });

  it('wraps every table on the page, not just the first', () => {
    const out = run('<table><tr><td>a</td></tr></table><table><tr><td>b</td></tr></table>');
    expect(out.match(/class="scroll-x"/g)).toHaveLength(2);
  });

  it('does not double-wrap a table that already has a scroll container', () => {
    const out = run('<div class="scroll-x" tabindex="0"><table><tr><td>a</td></tr></table></div>');
    expect(out.match(/class="scroll-x"/g)).toHaveLength(1);
  });

  it('preserves the surrounding content', () => {
    const out = run('<p>before</p><table><tr><td>a</td></tr></table><p>after</p>');
    expect(out).toContain('<p>before</p>');
    expect(out).toContain('<p>after</p>');
  });

  it('leaves a document with no table untouched', () => {
    expect(run('<p>nothing here</p>')).toBe('<p>nothing here</p>');
  });
});
