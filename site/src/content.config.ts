import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { DOCS } from './docs-map';

const docs = defineCollection({
  loader: glob({
    pattern: DOCS.map((d) => d.file),
    // The gitwho repo root, one level above this Astro project. The docs are
    // read in place rather than copied, so they cannot drift from the tool.
    base: '..',
    generateId: ({ entry }) => entry,
  }),
});

export const collections = { docs };
