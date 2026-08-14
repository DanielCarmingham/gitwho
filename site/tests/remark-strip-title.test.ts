import { describe, it, expect } from 'vitest';
import { unified } from 'unified';
import remarkParse from 'remark-parse';
import remarkStringify from 'remark-stringify';
import remarkStripTitle from '../src/plugins/remark-strip-title';

const run = (md: string) =>
  unified().use(remarkParse).use(remarkStripTitle).use(remarkStringify).processSync(md).toString();

describe('remarkStripTitle', () => {
  it('removes a leading level-1 heading', () => {
    expect(run('# Design\n\nWhy gitwho is shaped this way.\n')).not.toContain('# Design');
  });

  it('keeps the rest of the document intact', () => {
    expect(run('# Design\n\nWhy gitwho is shaped this way.\n')).toContain(
      'Why gitwho is shaped this way.',
    );
  });

  it('leaves a level-1 heading that is not first', () => {
    const out = run('Intro paragraph.\n\n# Not The Title\n');
    expect(out).toContain('# Not The Title');
    // Without this, an unconditional `children.shift()` — no type check, no
    // depth check — passes every test in this file while eating the first
    // paragraph of every document.
    expect(out).toContain('Intro paragraph.');
  });

  it('leaves deeper headings alone', () => {
    const out = run('# Design\n\n## Known limits\n');
    expect(out).toContain('## Known limits');
  });
});
