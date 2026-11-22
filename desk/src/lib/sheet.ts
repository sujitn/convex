import type { Measure, PublishedValuation } from '@convex/api-client'
import { addDecimal } from './decimal'

// The pricing sheet's model: one row a bond with a pricing rule, its desk mark when the
// desk quotes it, and its latest Live result. Pure, so the merge and the nudge are
// tested apart from the screen.

/** A level's quote field: a decimal yield, or basis points of spread or margin. */
export type LevelField = 'mid_yield' | 'mid_spread'

export type MethodKind = 'price' | 'yield' | 'benchmark_spread' | 'z_spread' | 'discount_margin' | 'oas'

/** How a bond is quoted, as its rule names it. */
export interface PricingMethod {
  kind: MethodKind
  /** The benchmark set member a benchmark spread is quoted to, such as `10Y`. */
  tenor?: string
}

/** The fields of a stored pricing rule the sheet reads. */
export interface PricingRule {
  instrument_id: string
  method: PricingMethod
  benchmark_set_id?: string | null
}

/** The fields of a stored bond the sheet reads. */
export interface Bond {
  instrument_id: string
  description: string
  issuer_name: string
  issuer_type: string
  bond_type: string
  currency: string
  coupon_rate: number | string | null
  maturity_date: string
  cusip: string | null
  isin: string | null
}

/** A desk mark as `GET /api/v1/market/levels` reads it back. */
export interface DeskMark {
  level: string
  marked_at: string
}

export interface DeskLevels {
  source: string
  levels: { instrument_id: string; field: LevelField; mark: DeskMark | null }[]
}

/** The way a row's clean price moved at its last result, and when that result arrived. */
export interface Tick {
  move: 'up' | 'down'
  at: number
}

export interface SheetRow {
  instrumentId: string
  bond: Bond | null
  rule: PricingRule
  /** The field the desk marks this bond's level in; none when the desk does not quote it. */
  deskField: LevelField | null
  mark: DeskMark | null
  valuation: PublishedValuation | null
  tick: Tick | null
}

/** The rule's method as a column label: `Price`, `Yield`, `Sprd 10Y`, `Z`, `DM`, `OAS`. */
export function methodLabel(method: PricingMethod): string {
  switch (method.kind) {
    case 'price':
      return 'Price'
    case 'yield':
      return 'Yield'
    case 'benchmark_spread':
      return `Sprd ${method.tenor ?? ''}`.trim()
    case 'z_spread':
      return 'Z'
    case 'discount_margin':
      return 'DM'
    case 'oas':
      return 'OAS'
  }
}

/**
 * The published measures that equal a level of each method, first found first: a
 * callable on its workouts publishes its yield to worst, a bullet its yield to maturity.
 * A benchmark spread is published by no measure.
 */
const LEVEL_MEASURES: Record<MethodKind, string[]> = {
  price: ['clean_price_mid'],
  yield: ['ytw_mid', 'ytm_mid'],
  benchmark_spread: [],
  z_spread: ['z_spread_mid'],
  discount_margin: ['discount_margin_mid'],
  oas: ['oas_mid'],
}

/** The unit a desk mark's field is in. */
const FIELD_UNIT: Record<LevelField, string> = { mid_yield: 'decimal_rate', mid_spread: 'bp' }

/**
 * A US Treasury note or bond, quoted and shown in 32nds. Bills are quoted on discount
 * and floating rate notes in decimals.
 */
export function isQuotedInThirtySeconds(bond: Bond | null): boolean {
  return bond?.issuer_type === 'Sovereign' && bond.currency === 'USD' && bond.bond_type === 'FixedBullet'
}

/** One basis point in a field's unit, or a quarter of one when `fine`. */
function nudgeStep(field: LevelField, fine: boolean): string {
  if (field === 'mid_yield') return fine ? '0.000025' : '0.0001'
  return fine ? '0.25' : '1'
}

/** `level` moved one nudge, exactly: up widens a spread or raises a yield. */
export function nudgedLevel(level: string, field: LevelField, direction: 1 | -1, fine = false): string {
  const step = nudgeStep(field, fine)
  return addDecimal(level, direction < 0 ? `-${step}` : step)
}

export function instrumentOf(valuation: PublishedValuation): string | undefined {
  return valuation.result.provenance.subject.instrument_id
}

/**
 * The newer of two results for one bond. Within a runtime the higher revision is newer;
 * a result from another runtime replaced the one before it, as after a restart.
 */
export function newerValuation(
  current: PublishedValuation | null,
  incoming: PublishedValuation,
): PublishedValuation {
  if (current === null || current.runtime_id !== incoming.runtime_id) return incoming
  return incoming.revision > current.revision ? incoming : current
}

export function measure(valuation: PublishedValuation | null, name: string): Measure | undefined {
  return valuation?.result.results[name]
}

function cleanPrice(valuation: PublishedValuation | null): number | null {
  const clean = measure(valuation, 'clean_price_mid')
  return clean ? Number(clean.value.value) : null
}

/** `row` with `incoming`, arrived at `receivedAt`, merged in, and the way its clean price moved. */
export function withValuation(row: SheetRow, incoming: PublishedValuation, receivedAt: number): SheetRow {
  const valuation = newerValuation(row.valuation, incoming)
  if (valuation === row.valuation) return row
  const before = cleanPrice(row.valuation)
  const after = cleanPrice(valuation)
  const tick: Tick | null =
    before === null || after === null || before === after ? null : { move: after > before ? 'up' : 'down', at: receivedAt }
  return { ...row, valuation, tick }
}

/** The level the Level column shows, with its unit: the desk's mark, or the measure that equals it. */
export function shownLevel(row: SheetRow): { value: string | number; unit: string } | null {
  if (row.deskField && row.mark) return { value: row.mark.level, unit: FIELD_UNIT[row.deskField] }
  for (const name of LEVEL_MEASURES[row.rule.method.kind]) {
    const published = measure(row.valuation, name)
    if (published) return published.value
  }
  return null
}

export type RowState = 'awaiting' | 'valid' | 'partial' | 'failed' | 'expired' | 'stale'

/**
 * The row's state from the server's own judgement: its result's status and quality,
 * and whether the result is past the `live_valid_until` the server stamped on it.
 */
export function rowState(valuation: PublishedValuation | null, now: number): RowState {
  if (valuation === null) return 'awaiting'
  if (valuation.result.status === 'Failed') return 'failed'
  if (valuation.live_valid_until && Date.parse(valuation.live_valid_until) < now) return 'expired'
  if (valuation.result.quality === 'Stale') return 'stale'
  return valuation.result.status === 'Partial' ? 'partial' : 'valid'
}

/**
 * One row a rule, in the server's instrument id order, with the desk's marks where it
 * quotes the bond.
 */
export function sheetRows(rules: PricingRule[], bonds: Bond[], desk: DeskLevels | null): SheetRow[] {
  const byId = new Map(bonds.map((bond) => [bond.instrument_id, bond]))
  const marks = new Map(desk?.levels.map((level) => [level.instrument_id, level]) ?? [])
  return rules.map((rule) => {
    const level = marks.get(rule.instrument_id)
    return {
      instrumentId: rule.instrument_id,
      bond: byId.get(rule.instrument_id) ?? null,
      rule,
      deskField: level?.field ?? null,
      mark: level?.mark ?? null,
      valuation: null,
      tick: null,
    }
  })
}

/**
 * `rows` with the desk's marks as read back, except the bonds the read may not reflect:
 * a mark of this page in flight, or taken after the read was issued. Every other bond
 * takes the read's mark, none included, as after a restart.
 */
export function withMarks(rows: SheetRow[], desk: DeskLevels, unread: ReadonlySet<string>): SheetRow[] {
  const marks = new Map(desk.levels.map((level) => [level.instrument_id, level]))
  let next = rows
  rows.forEach((row, index) => {
    const level = marks.get(row.instrumentId)
    if (!level || unread.has(row.instrumentId)) return
    const read = level.mark
    const isSame = read?.level === row.mark?.level && read?.marked_at === row.mark?.marked_at
    if (row.deskField === level.field && isSame) return
    if (next === rows) next = [...rows]
    next[index] = { ...row, deskField: level.field, mark: read }
  })
  return next
}
