import { parse as parseExact, stringify as stringifyExact } from 'lossless-json'

// The server compares numbers with their written scale, so `1.0` and `1` differ.
// JSON.parse/JSON.stringify would rewrite one as the other: anything sent back is
// either the user's own text or re-rendered here without touching numbers.

/** Parse a response keeping every number exactly as written. */
export function readExact(text: string): unknown {
  return parseExact(text)
}

/** Render a value read with `readExact`, numbers unchanged, indented for an editor. */
export function writeExact(value: unknown): string {
  return stringifyExact(value, null, 2) ?? ''
}

/**
 * A JSON object whose member values are JSON texts used verbatim, so a document or
 * fixture pasted by the operator reaches the server byte for byte.
 */
export function rawObject(members: Record<string, string>): string {
  return `{${Object.entries(members)
    .map(([name, text]) => `${JSON.stringify(name)}:${text}`)
    .join(',')}}`
}
