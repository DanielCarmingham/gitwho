import { existsSync } from 'node:fs';

/** A documentation page generated from the gitwho docs in this repo. */
export interface DocEntry {
  /** URL segment under /docs/. */
  slug: string;
  /** Path relative to the repo root. Also the content-collection entry id. */
  file: string;
  /** Page title. The source H1 is stripped and the layout renders this. */
  title: string;
  /** Sidebar label. */
  nav: string;
}

/**
 * Explicit rather than derived from filenames: a rename upstream must not
 * silently change a published URL.
 */
export const DOCS: DocEntry[] = [
  { slug: 'install', file: 'docs/INSTALL.md', title: 'Installing gitwho', nav: 'Install' },
  { slug: 'design', file: 'docs/DESIGN.md', title: 'Design', nav: 'Design' },
  { slug: 'security', file: 'SECURITY.md', title: 'Security', nav: 'Security' },
];

/**
 * Site routes for files the source markdown links to, keyed by basename.
 *
 * Keyed by basename because the same target is written differently depending
 * on which file links to it: INSTALL.md says `DESIGN.md`, the README says
 * `docs/DESIGN.md`. Both must resolve to one route.
 */
const ROUTES: Record<string, string> = {
  ...Object.fromEntries(DOCS.map((d) => [d.file.split('/').pop()!, `/docs/${d.slug}`])),
  'accounts.toml.example': '/docs/config',
};

/** The site route for a linked source file, or undefined if unknown. */
export function routeFor(basename: string): string | undefined {
  return ROUTES[basename];
}

/**
 * Fail the build rather than publish a site with no documentation.
 *
 * Called from the dynamic route at build time. These files live in the repo
 * root, one level up: the site is built from `site/`, the docs are not copied.
 * A miss here means a doc was renamed or removed without updating DOCS.
 *
 * @param exists Existence check, injectable for testing the throwing path
 *   without touching the filesystem. Defaults to the real `existsSync`.
 */
export function assertSourcesPresent(exists: (p: string) => boolean = existsSync): void {
  const missing = [...DOCS.map((d) => d.file), 'docs/accounts.toml.example'].filter(
    (file) => !exists(`../${file}`),
  );
  if (missing.length > 0) {
    throw new Error(
      `gitwho source files missing from the repo root: ${missing.join(', ')}\n` +
        `Each entry in DOCS must name a file that exists one level above site/.`,
    );
  }
}
