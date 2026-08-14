/** One run of a string: either prose, or something that should be set as code. */
export interface Segment {
  /** True when this run came from between a pair of backticks. */
  code: boolean;
  /** The text itself, with the backticks removed. */
  text: string;
}

/** A backtick pair with at least one character between them. */
const PAIR = /`([^`]+)`/g;

/**
 * Split a plain string into prose and code runs on backtick pairs.
 *
 * The recipe copy in `src/data/recipes.ts` is written the way the rest of this
 * project's prose is written — `includeIf "gitdir:"`, `match`, `sshKey` — but
 * it is data rendered through an Astro expression, not markdown, so nothing was
 * turning those pairs into anything. They reached the page as literal
 * backticks, in the sentence carrying the site's central argument.
 *
 * Returning segments rather than a string of HTML is the point. The caller maps
 * them to text nodes and `<code>` elements, so Astro escapes every text run
 * itself and there is no `set:html` anywhere near hand-written prose containing
 * quotation marks. A helper that returned markup would be one careless caller
 * away from an injection-shaped bug, even with static input.
 *
 * An unmatched backtick is left in place as ordinary text: it is neither thrown
 * on nor allowed to swallow the rest of the sentence, and `tests/recipes.test.ts`
 * fails the build on an odd count, so a typo is caught where it was written
 * rather than being quietly absorbed here.
 */
export function inlineCode(input: string): Segment[] {
  const segments: Segment[] = [];
  let cursor = 0;

  for (const match of input.matchAll(PAIR)) {
    const start = match.index;
    if (start > cursor) segments.push({ code: false, text: input.slice(cursor, start) });
    segments.push({ code: true, text: match[1] });
    cursor = start + match[0].length;
  }

  if (cursor < input.length) segments.push({ code: false, text: input.slice(cursor) });
  return segments;
}

/** Whether a string's backticks all pair up. False means a typo, not a style. */
export function backticksBalanced(input: string): boolean {
  return (input.match(/`/g) ?? []).length % 2 === 0;
}
