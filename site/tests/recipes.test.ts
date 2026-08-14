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
 * not: `deny_unknown_fields` is on `Defaults`, `Account` and `SourcedVar`
 * separately).
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
const SOURCED = fieldsOf('SourcedVar');

const named = (fields: Field[]) => fields.map((f) => f.key);
const requiredOf = (fields: Field[]) => fields.filter((f) => f.required).map((f) => f.key);

/** Every account in a recipe, parsed. */
type Account = Record<string, unknown> & { name?: string };
interface Parsed {
  defaults?: Record<string, unknown>;
  accounts?: Account[];
}

const parsed = (toml: string) => parse(toml) as Parsed;

/** `env` entries that are tables — the `SourcedVar` form. */
const sourcedEntries = (account: Account) =>
  ((account.env ?? []) as unknown[]).filter(
    (e): e is Record<string, unknown> => typeof e === 'object' && e !== null,
  );

/** `env` entries that are plain strings — `"VAR"` or `"VAR=literal"`. */
const simpleEntries = (account: Account) =>
  ((account.env ?? []) as unknown[]).filter((e): e is string => typeof e === 'string');

/**
 * The variables this account needs a value stored for.
 *
 * A bare `"VAR"` is a name to fetch from the store. `"VAR=literal"` carries its
 * own value. A table reads its value from another tool, so there is nothing to
 * store. `gitCredential` names a variable that git will need a password from,
 * so it has to be stored too — unless one of the other forms already supplies
 * it.
 */
function storedVariables(account: Account): Set<string> {
  const stored = new Set<string>();
  for (const entry of simpleEntries(account)) {
    if (!entry.includes('=')) stored.add(entry);
  }

  const credential = account.gitCredential as string | undefined;
  if (credential) {
    const supplied =
      sourcedEntries(account).some((e) => e.var === credential) ||
      simpleEntries(account).some((e) => e.startsWith(`${credential}=`));
    if (!supplied) stored.add(credential);
  }
  return stored;
}

/** `gitwho secret set <Account> <VAR>` — the only shape a recipe may list. */
const SECRET_SET = /^gitwho secret set (\S+) (\S+)$/;

describe('the schema this test reads from ../src/config.rs', () => {
  it('finds the three structs a recipe is parsed into', () => {
    expect(named(DEFAULTS).length).toBeGreaterThan(0);
    expect(named(ACCOUNT).length).toBeGreaterThan(0);
    expect(named(SOURCED).length).toBeGreaterThan(0);
  });

  it('tells a required field from one with a serde default', () => {
    // Anchors for the parser itself. `name` carries no attribute and `sshKey`
    // carries `#[serde(rename = "sshKey", default)]`; if the parser stopped
    // reading attributes, these are the two that would flip.
    expect(requiredOf(ACCOUNT)).toContain('name');
    expect(requiredOf(ACCOUNT)).not.toContain('sshKey');
    expect(requiredOf(DEFAULTS)).toContain('account');
    expect(requiredOf(DEFAULTS)).not.toContain('gitName');
  });

  it('reads the serde rename rather than the Rust field name', () => {
    expect(named(ACCOUNT)).toContain('gitCredential');
    expect(named(ACCOUNT)).toContain('match');
    expect(named(ACCOUNT)).not.toContain('git_credential');
    expect(named(ACCOUNT)).not.toContain('match_patterns');
  });

  it('confirms each struct still refuses an unknown field', () => {
    // Every "uses no key absent from the schema" check below is only load-
    // bearing because serde rejects the extra key at run time.
    for (const name of ['Defaults', 'Account', 'SourcedVar']) {
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
      for (const entry of sourcedEntries(account)) {
        for (const key of Object.keys(entry)) {
          expect(named(SOURCED), `example env table uses "${key}"`).toContain(key);
        }
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

  it('uses no key absent from SourcedVar inside an env table', () => {
    for (const recipe of RECIPES) {
      for (const account of parsed(recipe.toml).accounts ?? []) {
        for (const entry of sourcedEntries(account)) {
          const where = `${recipe.id} account "${account.name}" env table`;
          for (const key of Object.keys(entry)) {
            expect(named(SOURCED), `${where} uses "${key}"`).toContain(key);
          }
          for (const key of requiredOf(SOURCED)) {
            expect(Object.keys(entry), `${where} must set "${key}"`).toContain(key);
          }
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

  it('lists a secret command for every variable the config needs stored', () => {
    for (const recipe of RECIPES) {
      const listed = new Set(
        recipe.secrets
          .map((line) => line.match(SECRET_SET))
          .filter((m): m is RegExpMatchArray => m !== null)
          .map((m) => `${m[1]}/${m[2]}`),
      );
      for (const account of parsed(recipe.toml).accounts ?? []) {
        for (const variable of storedVariables(account)) {
          expect(listed, `${recipe.id} needs "gitwho secret set ${account.name} ${variable}"`).toContain(
            `${account.name}/${variable}`,
          );
        }
      }
    }
  });

  it('names an account and a variable that config actually declares in every secret command', () => {
    for (const recipe of RECIPES) {
      const accounts = parsed(recipe.toml).accounts ?? [];
      for (const line of recipe.secrets) {
        const match = line.match(SECRET_SET);
        expect(match, `${recipe.id}: "${line}" is not a "gitwho secret set <Account> <VAR>"`).not.toBeNull();
        const [, accountName, variable] = match!;

        const account = accounts.find((a) => a.name === accountName);
        expect(account, `${recipe.id} has no account named "${accountName}"`).toBeDefined();
        expect(
          [...storedVariables(account!)],
          `${recipe.id}: account "${accountName}" never declares "${variable}"`,
        ).toContain(variable);
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
      for (const line of recipe.secrets) {
        expect(line, `${recipe.id} secrets`).not.toContain('`');
      }
    }
  });

  it('never embeds a token value', () => {
    for (const recipe of RECIPES) {
      expect(recipe.toml).not.toMatch(/gh[pousr]_[A-Za-z0-9]{16,}/);
    }
  });
});
