import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { parse } from 'smol-toml';
import { RECIPES } from '../src/data/recipes';

const example = readFileSync('../docs/accounts.toml.example', 'utf8');

/** Top-level and per-account keys the upstream example demonstrates. */
const knownKeys = new Set(
  example
    .split('\n')
    .map((line) => line.match(/^\s*#?\s*([A-Za-z][A-Za-z0-9]*)\s*=/))
    .filter((m): m is RegExpMatchArray => m !== null)
    .map((m) => m[1]),
);

describe('RECIPES', () => {
  it('ships the four documented shapes', () => {
    expect(RECIPES).toHaveLength(4);
  });

  it('parses every recipe as valid TOML', () => {
    for (const recipe of RECIPES) {
      expect(() => parse(recipe.toml), `${recipe.id} must parse`).not.toThrow();
    }
  });

  it('uses no key absent from the upstream example', () => {
    for (const recipe of RECIPES) {
      const parsed = parse(recipe.toml) as Record<string, unknown>;
      const accounts = (parsed.accounts ?? []) as Record<string, unknown>[];
      const used = [
        ...Object.keys((parsed.defaults ?? {}) as object),
        ...accounts.flatMap((a) => Object.keys(a)),
      ];
      for (const key of used) {
        expect(knownKeys, `${recipe.id} uses unknown key "${key}"`).toContain(key);
      }
    }
  });

  it('declares a defaults account that one of its accounts defines', () => {
    for (const recipe of RECIPES) {
      const parsed = parse(recipe.toml) as any;
      const names = (parsed.accounts ?? []).map((a: any) => a.name);
      expect(names, `${recipe.id} defaults.account must exist`).toContain(parsed.defaults.account);
    }
  });

  it('never embeds a token value', () => {
    for (const recipe of RECIPES) {
      expect(recipe.toml).not.toMatch(/gh[pousr]_[A-Za-z0-9]{16,}/);
    }
  });
});
