import { describe, it, expect } from 'vitest';
import { inlineCode, backticksBalanced } from '../src/inline-code';

describe('inlineCode', () => {
  it('passes a plain string through as one prose segment', () => {
    expect(inlineCode('nothing to mark up here')).toEqual([
      { code: false, text: 'nothing to mark up here' },
    ]);
  });

  it('turns a single backtick pair into a code segment', () => {
    expect(inlineCode('run `match` first')).toEqual([
      { code: false, text: 'run ' },
      { code: true, text: 'match' },
      { code: false, text: ' first' },
    ]);
  });

  it('handles several pairs in one string', () => {
    expect(inlineCode('`match` runs against `host/path`, so')).toEqual([
      { code: true, text: 'match' },
      { code: false, text: ' runs against ' },
      { code: true, text: 'host/path' },
      { code: false, text: ', so' },
    ]);
  });

  it('keeps the quotation marks inside a span, unescaped and unaltered', () => {
    // The caller renders these as text nodes, so escaping is Astro's job, not
    // this function's. It must not mangle them on the way through.
    expect(inlineCode('the case `includeIf "gitdir:"` loses')[1]).toEqual({
      code: true,
      text: 'includeIf "gitdir:"',
    });
  });

  it('leaves an unmatched trailing backtick as text rather than throwing', () => {
    expect(() => inlineCode('a stray ` backtick here')).not.toThrow();
    expect(inlineCode('a stray ` backtick here')).toEqual([
      { code: false, text: 'a stray ` backtick here' },
    ]);
  });

  it('does not swallow the rest of the sentence after an unmatched backtick', () => {
    const out = inlineCode('`match` is fine but this ` one is not');
    expect(out.map((s) => s.text).join('')).toContain('one is not');
    expect(out.filter((s) => s.code)).toEqual([{ code: true, text: 'match' }]);
  });

  it('reassembles to the original text with the paired backticks removed', () => {
    const source = 'one new `[[accounts]]` block, then `gitwho init --write`';
    expect(inlineCode(source).map((s) => s.text).join('')).toBe(
      source.replace(/`/g, ''),
    );
  });

  it('leaves an empty pair alone rather than emitting an empty code segment', () => {
    expect(inlineCode('an empty `` pair')).toEqual([{ code: false, text: 'an empty `` pair' }]);
  });

  it('returns nothing for an empty string', () => {
    expect(inlineCode('')).toEqual([]);
  });
});

describe('backticksBalanced', () => {
  it('accepts a string with no backticks', () => {
    expect(backticksBalanced('plain prose')).toBe(true);
  });

  it('accepts paired backticks', () => {
    expect(backticksBalanced('`match` and `host/path`')).toBe(true);
  });

  it('rejects an odd count', () => {
    expect(backticksBalanced('`match and host/path')).toBe(false);
  });
});
