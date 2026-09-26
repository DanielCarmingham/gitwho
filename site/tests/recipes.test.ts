import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { parse } from 'smol-toml';
import { RECIPES } from '../src/data/recipes';
import { backticksBalanced } from '../src/inline-code';

/*
 * The recipes are the only configs on this site a reader is invited to paste
 * whole, so a schema change in the tool that this file does not notice ships as
 * a config that fails on the reader's machine after they have committed effort.
 *
 * The schema is therefore read from `../src/config.rs`, which is the schema —
 * not from `../docs/accounts.toml.example`, which is one *illustration* of it.
 * The example cannot answer either of the two questions that matter here: which
 * keys are required (it fills all of them in), and which section a key belongs
 * to (a flat scan of it merges `[defaults]` with `[[accounts]]`, and serde does
 * not: `deny_unknown_fields` is on `Defaults` and `Account` separately).
 */
const CONFIG_RS = readFileSync('../src/config.rs', 'utf8');
const EXAMPLE = readFileSync('../docs/accounts.toml.example', 'utf8');

interface Field {
  /** The name as written in TOML — the `serde(rename)` when there is one. */
  key: string;
  /** No `#[serde(default)]`, so serde refuses a document that omits it. */
  required: boolean;
}

/** The body of `pub struct <name> { … }`, plus the derive attributes above it. */
function structOf(name: string): { attrs: string; body: string } {
  const start = CONFIG_RS.indexOf(`pub struct ${name} {`);
  if (start === -1) {
    throw new Error(
      `../src/config.rs has no \`pub struct ${name}\`. It was renamed or moved; ` +
        `this test derives the site's schema from it and cannot guess.`,
    );
  }
  const end = CONFIG_RS.indexOf('\n}', start);
  if (end === -1) throw new Error(`Could not find the end of \`pub struct ${name}\`.`);

  const derive = CONFIG_RS.lastIndexOf('#[derive', start);
  return {
    attrs: derive === -1 ? '' : CONFIG_RS.slice(derive, start),
    body: CONFIG_RS.slice(start, end),
  };
}

/**
 * The TOML field set of a serde struct in `../src/config.rs`.
 *
 * A hand-rolled reader rather than anything clever: the file is `cargo fmt`ed,
 * so a field is `pub <name>: <type>,` preceded by its attributes, and a
 * `#[serde(rename = "…")]` renames it. Throwing on a struct that yields no
 * fields is the point — a parser that quietly returned an empty set would turn
 * every check below into a no-op.
 */
function fieldsOf(name: string): Field[] {
  const { body } = structOf(name);
  const fields: Field[] = [];
  let attrs = '';

  for (const line of body.split('\n').slice(1)) {
    const text = line.trim();
    if (text === '' || text.startsWith('//')) continue;
    if (text.startsWith('#[')) {
      attrs += text;
      continue;
    }
    const match = text.match(/^pub\s+([a-z_0-9]+)\s*:/);
    if (match) {
      const renamed = attrs.match(/rename\s*=\s*"([^"]+)"/);
      fields.push({ key: renamed ? renamed[1] : match[1], required: !/\bdefault\b/.test(attrs) });
    }
    attrs = '';
  }

  if (fields.length === 0) {
    throw new Error(`Parsed no fields out of \`pub struct ${name}\` in ../src/config.rs.`);
  }
  return fields;
}

const DEFAULTS = fieldsOf('Defaults');
const ACCOUNT = fieldsOf('Account');

const named = (fields: Field[]) => fields.map((f) => f.key);
const requiredOf = (fields: Field[]) => fields.filter((f) => f.required).map((f) => f.key);

/** Every account in a recipe, parsed. */
type Account = Record<string, unknown> & {
  name?: string;
  provider?: string;
  url?: string;
  login?: string;
};
interface Parsed {
  defaults?: Record<string, unknown>;
  accounts?: Account[];
}

const parsed = (toml: string) => parse(toml) as Parsed;

/** A `gh auth login` or `tea login add` line, exactly as a reader would type it. */
const GH_LOGIN = /^gh auth login --hostname (\S+)(?:\s+#.*)?$/;
const TEA_LOGIN = /^tea login add --url (\S+)(?:\s+#.*)?$/;

describe('the schema this test reads from ../src/config.rs', () => {
  it('finds the two structs a recipe is parsed into', () => {
    expect(named(DEFAULTS).length).toBeGreaterThan(0);
    expect(named(ACCOUNT).length).toBeGreaterThan(0);
  });

  it('tells a required field from one with a serde default', () => {
    // Anchors for the parser itself. `login` carries no attribute and `sshKey`
    // carries `#[serde(rename = "sshKey", default)]`; if the parser stopped
    // reading attributes, these are the two that would flip.
    expect(requiredOf(ACCOUNT)).toContain('login');
    expect(requiredOf(ACCOUNT)).not.toContain('sshKey');
    expect(requiredOf(DEFAULTS)).toContain('account');
    expect(requiredOf(DEFAULTS)).not.toContain('gitName');
  });

  it('reads the serde rename rather than the Rust field name', () => {
    expect(named(ACCOUNT)).toContain('sshKey');
    expect(named(ACCOUNT)).toContain('match');
    expect(named(ACCOUNT)).not.toContain('ssh_key');
    expect(named(ACCOUNT)).not.toContain('match_patterns');
  });

  it('confirms each struct still refuses an unknown field', () => {
    // Every "uses no key absent from the schema" check below is only load-
    // bearing because serde rejects the extra key at run time.
    for (const name of ['Defaults', 'Account']) {
      expect(structOf(name).attrs, `${name} must deny unknown fields`).toContain(
        'deny_unknown_fields',
      );
    }
  });

  it('accepts every key the upstream example uses, in the section it uses it', () => {
    // Cross-check in the other direction: the tool's own example must satisfy
    // the field sets parsed out of the tool's own parser.
    const example = parsed(EXAMPLE);
    for (const key of Object.keys(example.defaults ?? {})) {
      expect(named(DEFAULTS), `example [defaults] uses "${key}"`).toContain(key);
    }
    for (const account of example.accounts ?? []) {
      for (const key of Object.keys(account)) {
        expect(named(ACCOUNT), `example [[accounts]] uses "${key}"`).toContain(key);
      }
    }
  });
});

describe('RECIPES', () => {
  it('ships the four documented shapes', () => {
    expect(RECIPES).toHaveLength(4);
  });

  it('parses every recipe as valid TOML', () => {
    for (const recipe of RECIPES) {
      expect(() => parse(recipe.toml), `${recipe.id} must parse`).not.toThrow();
    }
  });

  it('uses no top-level key beyond defaults and accounts', () => {
    for (const recipe of RECIPES) {
      for (const key of Object.keys(parsed(recipe.toml))) {
        expect(['defaults', 'accounts'], `${recipe.id} has top-level "${key}"`).toContain(key);
      }
    }
  });

  it('puts every [defaults] key in Defaults, not in Account', () => {
    for (const recipe of RECIPES) {
      const defaults = parsed(recipe.toml).defaults ?? {};
      for (const key of Object.keys(defaults)) {
        expect(named(DEFAULTS), `${recipe.id} [defaults] uses "${key}"`).toContain(key);
      }
    }
  });

  it('puts every [[accounts]] key in Account, not in Defaults', () => {
    for (const recipe of RECIPES) {
      for (const account of parsed(recipe.toml).accounts ?? []) {
        for (const key of Object.keys(account)) {
          expect(named(ACCOUNT), `${recipe.id} account "${account.name}" uses "${key}"`).toContain(
            key,
          );
        }
      }
    }
  });

  it('supplies every field serde has no default for', () => {
    for (const recipe of RECIPES) {
      const defaults = parsed(recipe.toml).defaults ?? {};
      for (const key of requiredOf(DEFAULTS)) {
        expect(Object.keys(defaults), `${recipe.id} [defaults] must set "${key}"`).toContain(key);
      }
      for (const account of parsed(recipe.toml).accounts ?? []) {
        for (const key of requiredOf(ACCOUNT)) {
          const where = `${recipe.id} account "${account.name ?? '?'}" must set "${key}"`;
          expect(Object.keys(account), where).toContain(key);
        }
      }
    }
  });

  it('declares a defaults account that one of its accounts defines', () => {
    for (const recipe of RECIPES) {
      const config = parsed(recipe.toml);
      const names = (config.accounts ?? []).map((a) => a.name);
      expect(names, `${recipe.id} defaults.account must exist`).toContain(config.defaults?.account);
    }
  });

  it('gives every gitea account a url, and no github account one', () => {
    for (const recipe of RECIPES) {
      for (const account of parsed(recipe.toml).accounts ?? []) {
        if (account.provider === 'gitea') {
          expect(account.url, `${recipe.id} account "${account.name}" (gitea) needs "url"`).toBeTruthy();
        } else if (account.provider === 'github') {
          expect(
            account.url,
            `${recipe.id} account "${account.name}" (github) must not set "url"`,
          ).toBeUndefined();
        }
      }
    }
  });

  it('lists one login command per account, matching its provider', () => {
    for (const recipe of RECIPES) {
      const accounts = parsed(recipe.toml).accounts ?? [];
      expect(recipe.logins, `${recipe.id} lists one login line per account`).toHaveLength(
        accounts.length,
      );

      for (const line of recipe.logins) {
        const gh = line.match(GH_LOGIN);
        const tea = line.match(TEA_LOGIN);
        expect(gh ?? tea, `${recipe.id}: "${line}" is not a gh or tea login command`).not.toBeNull();
      }

      for (const account of accounts) {
        if (account.provider === 'github') {
          expect(
            recipe.logins.some((line) => GH_LOGIN.test(line) && line.includes(`github.com`)),
            `${recipe.id}: no "gh auth login --hostname github.com" for "${account.name}"`,
          ).toBe(true);
        } else if (account.provider === 'gitea') {
          expect(
            recipe.logins.some((line) => {
              const match = line.match(TEA_LOGIN);
              return match !== undefined && match !== null && match[1] === account.url;
            }),
            `${recipe.id}: no "tea login add --url ${account.url}" for "${account.name}"`,
          ).toBe(true);
        }
      }
    }
  });

  it('pairs every backtick in the prose that gets marked up', () => {
    // inlineCode leaves an unmatched backtick as text rather than throwing, so
    // this is where a typo has to be caught: an odd count would otherwise ship
    // a stray character into the sentence, which is the defect this markup was
    // added to remove.
    for (const recipe of RECIPES) {
      expect(backticksBalanced(recipe.problem), `${recipe.id} problem`).toBe(true);
    }
  });

  it('keeps backticks out of the strings that are not marked up', () => {
    // Only `problem` goes through inlineCode. A backtick anywhere else renders
    // literally -- in a heading, in the table of contents, or inside a shell
    // command a reader is meant to paste.
    for (const recipe of RECIPES) {
      expect(recipe.title, `${recipe.id} title`).not.toContain('`');
      for (const line of recipe.logins) {
        expect(line, `${recipe.id} logins`).not.toContain('`');
      }
    }
  });

  it('never embeds a token value', () => {
    for (const recipe of RECIPES) {
      expect(recipe.toml).not.toMatch(/gh[pousr]_[A-Za-z0-9]{16,}/);
    }
  });
});
