import type { DocEntry } from './docs-map';
import type { Recipe } from './data/recipes';

export interface SearchDocument {
  title: string;
  href: string;
  body: string;
}

interface MarkdownSource {
  doc: DocEntry;
  raw: string;
}

interface SearchSources {
  markdown: MarkdownSource[];
  configSource: string;
  recipes: Recipe[];
}

export function normalizeSearchText(source: string): string {
  return source
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/```[a-zA-Z0-9_-]*\n/g, '')
    .replace(/```/g, '')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/[`*#[\]()]/g, ' ')
    .replace(/["{}[\],]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

export function buildSearchDocuments({
  markdown,
  configSource,
  recipes,
}: SearchSources): SearchDocument[] {
  return [
    ...markdown.map(({ doc, raw }) => ({
      title: doc.title,
      href: `/docs/${doc.slug}/`,
      body: normalizeSearchText(raw),
    })),
    {
      title: 'Config',
      href: '/docs/config/',
      body: normalizeSearchText(configSource),
    },
    {
      title: 'Recipes',
      href: '/docs/recipes/',
      body: normalizeSearchText(
        recipes
          .map((recipe) => [recipe.title, recipe.problem, recipe.toml, ...recipe.logins].join('\n'))
          .join('\n\n'),
      ),
    },
  ];
}

export function serializeSearchDocuments(documents: SearchDocument[]): string {
  return JSON.stringify(documents).replace(/</g, '\\u003c');
}
