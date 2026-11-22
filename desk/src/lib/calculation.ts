import { isDecimal } from './decimal'

// The calculator's reply is the bond's quote with its measures as flat members, each a
// number or null, beside what was read: the input, the rule, the workouts and the
// quote's own record (its instrument, timestamp and quality score).

/** Numeric members of the reply that describe the quote, not the bond. */
const QUOTE_RECORD = new Set(['quality', 'timestamp'])

/** The reply's measures, in its order: every numeric member but the quote's own record. */
export function calculatedMeasures(reply: Record<string, unknown>): [string, number | string][] {
  return Object.entries(reply).filter(
    (entry): entry is [string, number | string] =>
      !QUOTE_RECORD.has(entry[0]) &&
      (typeof entry[1] === 'number' || (typeof entry[1] === 'string' && isDecimal(entry[1]))),
  )
}
