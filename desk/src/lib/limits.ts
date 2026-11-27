import { codeWords, formatAmount, formatPercent, formatYears } from './format'

// A book's limits as the status route, the pre-trade check and the `book` topic give them,
// and how the desk words and colours them.

/** A limit's status, from the least severe to the most, as the server orders them. */
const LIMIT_STATUSES = ['within', 'warning', 'unknown', 'breach'] as const
export type LimitStatus = (typeof LIMIT_STATUSES)[number]

/** Why a limit, or a book's risk measure, is not known. */
export interface UnknownReason {
  code: string
  /** The positions it applies to: the first few, and how many in all. */
  instruments?: { count: number; first: string[] }
  instrument?: string
  measure?: string
  detail?: string
}

/** The members a limit bounds: each of them, or one. */
export type LimitMember = 'each' | { one: string }

/** What a limit bounds. */
export type LimitMeasure =
  | { kind: 'face'; instrument: LimitMember }
  | { kind: 'market_value' | 'gross_market_value' | 'dv01' | 'cs01' | 'modified_duration' }
  | { kind: 'key_rate_dv01'; tenor: string }
  | { kind: 'group_market_value' | 'group_share'; by: string; group: LimitMember }

/**
 * One limit evaluated, as the status route and the pre-trade check give it: what it
 * bounds, the bounds it was judged against (an excess's while one applies), its status,
 * value and use, the member that set the status, and why it is unknown.
 */
export interface LimitEvaluation {
  limit_id: string
  measure: LimitMeasure
  lower?: string
  upper?: string
  status: LimitStatus
  value?: string
  member?: string
  utilisation?: string
  reason?: UnknownReason
}

/** How many limits are in each status. */
export type StatusCounts = Partial<Record<LimitStatus, number>>

/** How many of `statuses` are in each status. */
export function countStatuses(statuses: Iterable<LimitStatus>): StatusCounts {
  const counts: StatusCounts = {}
  for (const status of statuses) counts[status] = (counts[status] ?? 0) + 1
  return counts
}

/** The most severe status counted; none when nothing is. */
export function worstCounted(counts: StatusCounts): LimitStatus | null {
  for (let index = LIMIT_STATUSES.length - 1; index >= 0; index -= 1) {
    const status = LIMIT_STATUSES[index]!
    if ((counts[status] ?? 0) > 0) return status
  }
  return null
}

/** Each status with its count, the most severe first, as a cell reads them. */
export function countsSummary(counts: StatusCounts): string {
  const counted = [...LIMIT_STATUSES]
    .reverse()
    .filter((status) => (counts[status] ?? 0) > 0)
    .map((status) => `${status} ${counts[status]}`)
  return counted.length > 0 ? counted.join(' · ') : 'no limits'
}

/** The colour a status is written in. */
export const LIMIT_TONE: Record<LimitStatus, string> = {
  within: 'text-emerald-400',
  warning: 'text-amber-300',
  unknown: 'text-amber-300',
  breach: 'text-red-400',
}

/** The colour a limit's use is drawn in, by its status. */
export const LIMIT_FILL: Record<LimitStatus, string> = {
  within: 'bg-emerald-500',
  warning: 'bg-amber-400',
  unknown: 'bg-slate-600',
  breach: 'bg-red-500',
}

/** A limit's member, as a label reads it. */
function memberLabel(member: LimitMember): string {
  return member === 'each' ? 'each' : member.one
}

/** What a limit bounds, as a desk names it. */
export function measureLabel(measure: LimitMeasure): string {
  switch (measure.kind) {
    case 'face':
      return `Face · ${memberLabel(measure.instrument)}`
    case 'market_value':
      return 'Market value'
    case 'gross_market_value':
      return 'Gross market value'
    case 'dv01':
      return 'DV01'
    case 'cs01':
      return 'CS01'
    case 'modified_duration':
      return 'Modified duration'
    case 'key_rate_dv01':
      return `Key-rate DV01 ${measure.tenor}`
    case 'group_market_value':
      return `${measure.by} market value · ${memberLabel(measure.group)}`
    case 'group_share':
      return `${measure.by} share · ${memberLabel(measure.group)}`
  }
}

/** A limit's value or bound in its measure's unit: a share in percent, a duration in years, else whole units. */
export function formatLimitValue(measure: LimitMeasure, value: string): string {
  if (measure.kind === 'group_share') return formatPercent(value)
  if (measure.kind === 'modified_duration') return formatYears(value)
  return formatAmount(value)
}

/** A limit's bounds, as a cell reads them. */
export function boundsLabel({ measure, lower, upper }: Pick<LimitEvaluation, 'measure' | 'lower' | 'upper'>): string {
  const shown = (bound: string) => formatLimitValue(measure, bound)
  if (lower !== undefined && upper !== undefined) return `${shown(lower)} to ${shown(upper)}`
  if (upper !== undefined) return `≤ ${shown(upper)}`
  if (lower !== undefined) return `≥ ${shown(lower)}`
  return 'unbounded'
}

/** Why a limit or measure is not known, in a line. */
export function unknownReasonText(reason: UnknownReason): string {
  const parts = [codeWords(reason.code)]
  if (reason.measure) parts.push(reason.measure)
  if (reason.instruments) {
    const { count, first } = reason.instruments
    parts.push(`${count} position${count === 1 ? '' : 's'} (${first.join(', ')}${count > first.length ? ', …' : ''})`)
  }
  if (reason.instrument) parts.push(reason.instrument)
  if (reason.detail) parts.push(reason.detail)
  return parts.join(': ')
}
