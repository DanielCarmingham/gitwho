import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { DOCS, routeFor, assertSourcesPresent } from '../src/docs-map';

describe('DOCS', () => {
  it('lists exactly the three markdown documents', () => {
    expect(DOCS.map((d) => d.slug).sort()).toEqual(['design', 'install', 'security']);
  });

  it('points every entry at a file that exists in the submodule', () => {
    for (const doc of DOCS) {
      expect(() => readFileSync(`../${doc.file}`, 'utf8')).not.toThrow();
    }
  });

  it('maps a source basename to its site route', () => {
    expect(routeFor('DESIGN.md')).toBe('/docs/design');
    expect(routeFor('INSTALL.md')).toBe('/docs/install');
    expect(routeFor('SECURITY.md')).toBe('/docs/security');
    expect(routeFor('accounts.toml.example')).toBe('/docs/config');
  });

  it('returns undefined for a basename it does not know', () => {
    expect(routeFor('CONTRIBUTING.md')).toBeUndefined();
  });
});

describe('assertSourcesPresent', () => {
  it('does not throw when every source file is present', () => {
    expect(() => assertSourcesPresent(() => true)).not.toThrow();
  });

  it('throws naming the missing file and where it was expected', () => {
    const exists = (p: string) => p !== '../SECURITY.md';
    expect(() => assertSourcesPresent(exists)).toThrow(
      /SECURITY\.md.*\n.*one level above site\//s,
    );
  });

  it('looks for sources one level above the site, not inside it', () => {
    const seen: string[] = [];
    assertSourcesPresent((p) => {
      seen.push(p);
      return true;
    });
    expect(seen).toContain('../SECURITY.md');
    expect(seen).toContain('../docs/accounts.toml.example');
    expect(seen.every((p) => p.startsWith('../'))).toBe(true);
  });
});
