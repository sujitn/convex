// A limit's status, as the RFQ blotter's check and the book screen read it.

/** A limit's status, from the least severe to the most, as the server orders them. */
export const LIMIT_STATUSES = ['within', 'warning', 'unknown', 'breach'] as const
export type LimitStatus = (typeof LIMIT_STATUSES)[number]

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

/** The colour a status is shown in. */
export const LIMIT_TONE: Record<LimitStatus, string> = {
  within: 'text-emerald-400',
  warning: 'text-amber-300',
  unknown: 'text-amber-300',
  breach: 'text-red-400',
}
