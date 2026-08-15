import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const layout = readFileSync('src/layouts/DocsLayout.astro', 'utf8');

describe('DocsLayout desktop width', () => {
  it('gives the docs body a flexible desktop column instead of the prose measure', () => {
    expect(layout).toContain('@media (min-width: 76rem)');
    expect(layout).toContain('grid-template-columns: 13rem minmax(0, 1fr) 14rem;');
    expect(layout).not.toContain('@media (min-width: 64rem)');
    expect(layout).not.toContain('grid-template-columns: 11rem minmax(0, var(--measure)) 11rem;');
  });

  it('lets code and config blocks use the full docs column while constraining prose text', () => {
    expect(layout).toMatch(/\.docs__content\s*{[^}]*max-width:\s*none;/s);
    expect(layout).toContain('.docs__content :global(:is(p, ul, ol, blockquote))');
    expect(layout).toMatch(/max-width:\s*var\(--measure\);/);
  });
});
