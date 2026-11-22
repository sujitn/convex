import type { PublishedValuation } from '@convex/api-client'
import { describe, expect, it } from 'vitest'
import { addDecimal, isDecimal, multiplyDecimal } from './decimal'
import { formatAge, formatCoupon, formatMeasure, formatThirtySeconds, measureUnit } from './format'
import {
  levelField,
  methodLabel,
  newerValuation,
  nudgedLevel,
  rowState,
  sheetRows,
  shownLevel,
  withValuation,
  type Bond,
  type PricingRule,
} from './sheet'
import { streamProtocols, subscriptionMessages } from './stream'

function valuation(
  revision: number,
  clean: string,
  overrides: { runtime?: string; status?: string; quality?: string; validUntil?: string | null } = {},
): PublishedValuation {
  return {
    runtime_id: overrides.runtime ?? 'R1',
    revision,
    generation: revision,
    live_valid_until: overrides.validUntil ?? null,
    result: {
      request_id: `r-${revision}`,
      status: overrides.status ?? 'Complete',
      quality: overrides.quality ?? 'Valid',
      configuration_version: 1,
      results: {
        clean_price_mid: { value: { value: clean, unit: 'price_per_100' }, quality: 'Valid', method: 'm' },
        ytm_mid: { value: { value: 0.0525, unit: 'decimal_rate' }, quality: 'Valid', method: 'm' },
      },
      errors: [],
      provenance: { subject: { instrument_id: 'BOND-001', version: 1 } },
    },
  }
}

const corporate: Bond = {
  instrument_id: 'BOND-001',
  description: 'ACME 5.25 2035',
  issuer_name: 'ACME',
  issuer_type: 'Corporate',
  currency: 'USD',
  coupon_rate: 0.0525,
  maturity_date: '2035-06-15',
  cusip: '000000AA0',
  isin: null,
}

describe('exact decimals', () => {
  it('adds and multiplies at the larger scale, never through a float', () => {
    expect(addDecimal('0.0525', '0.0001')).toBe('0.0526')
    expect(addDecimal('0.1', '0.2')).toBe('0.3')
    expect(addDecimal('125', '-0.25')).toBe('124.75')
    expect(addDecimal('0.5', '-1')).toBe('-0.5')
    expect(addDecimal('-0.0001', '0.0001')).toBe('0.0000')
    expect(multiplyDecimal('0.0001', -3)).toBe('-0.0003')
    expect(() => multiplyDecimal('1', 0.5)).toThrow()
  })
  it('takes only plain decimals', () => {
    for (const text of ['125', '-0.25', '0.0525']) expect(isDecimal(text)).toBe(true)
    for (const text of ['', '1e3', '1.', '.5', '1,000', ' 1', '+1']) expect(isDecimal(text)).toBe(false)
  })
})

describe('nudges', () => {
  it('move a spread by a basis point or a quarter, and a yield by a basis point of yield', () => {
    expect(nudgedLevel('125', 'mid_spread', 1)).toBe('126')
    expect(nudgedLevel('125', 'mid_spread', -1, true)).toBe('124.75')
    expect(nudgedLevel('0.0525', 'mid_yield', 2)).toBe('0.0527')
    expect(nudgedLevel('0.0525', 'mid_yield', -1, true)).toBe('0.052475')
  })
  it('read each method from its field', () => {
    expect(levelField({ kind: 'yield' })).toBe('mid_yield')
    expect(levelField({ kind: 'benchmark_spread', tenor: '10Y' })).toBe('mid_spread')
    expect(levelField({ kind: 'oas' })).toBe('mid_spread')
    expect(levelField({ kind: 'price' })).toBeNull()
    expect(methodLabel({ kind: 'benchmark_spread', tenor: '10Y' })).toBe('Sprd 10Y')
  })
})

describe('formats', () => {
  it('show Treasury prices in 32nds, halves and eighths', () => {
    expect(formatThirtySeconds('99.5')).toBe('99-16')
    expect(formatThirtySeconds('99.515625')).toBe('99-16+')
    expect(formatThirtySeconds('99.5078125')).toBe('99-162')
    expect(formatThirtySeconds(100)).toBe('100-00')
    expect(formatThirtySeconds('98.03125')).toBe('98-01')
  })
  it('show yields in percent and spreads in basis points', () => {
    expect(formatMeasure(0.0525, 'decimal_rate')).toBe('5.250')
    expect(formatMeasure(125.04, 'bp')).toBe('125.0')
    expect(formatMeasure('99.24373766953476661001332101', 'price_per_100')).toBe('99.244')
    expect(formatMeasure('99.5', 'price_per_100', true)).toBe('99-16')
    expect(formatCoupon(0.04125)).toBe('4.125')
    expect(formatCoupon(null)).toBe('—')
  })
  it('read a flat measure unit from its name', () => {
    expect(measureUnit('clean_price_mid')).toBe('price_per_100')
    expect(measureUnit('accrued_interest')).toBe('price_per_100')
    expect(measureUnit('ytm_mid')).toBe('decimal_rate')
    expect(measureUnit('z_spread_mid')).toBe('bp')
    expect(measureUnit('discount_margin_mid')).toBe('bp')
    expect(measureUnit('oas_mid')).toBe('bp')
    expect(measureUnit('modified_duration_mid')).toBeNull()
  })
  it('show a mark age coarsely', () => {
    const now = Date.parse('2026-11-22T12:00:00Z')
    expect(formatAge('2026-11-22T11:59:30Z', now)).toBe('30s')
    expect(formatAge('2026-11-22T11:45:00Z', now)).toBe('15m')
    expect(formatAge('2026-11-22T09:00:00Z', now)).toBe('3h')
  })
})

describe('results', () => {
  it('keep the newer by revision within a runtime, and a new runtime replaces the old', () => {
    const current = valuation(5, '99.5')
    expect(newerValuation(current, valuation(4, '99'))).toBe(current)
    expect(newerValuation(current, valuation(6, '99')).revision).toBe(6)
    expect(newerValuation(current, valuation(1, '98', { runtime: 'R2' })).runtime_id).toBe('R2')
    expect(newerValuation(null, current)).toBe(current)
  })
  it('colour a row by the way its clean price moved, and ignore an older result', () => {
    const [row] = sheetRows([{ instrument_id: 'BOND-001', method: { kind: 'price' } }], [corporate], null)
    const first = withValuation(row!, valuation(1, '99.5'), 10)
    expect(first.move).toBeNull()
    const down = withValuation(first, valuation(2, '99.25'), 20)
    expect([down.move, down.movedAt]).toEqual(['down', 20])
    expect(withValuation(down, valuation(1, '100'), 30)).toBe(down)
    expect(withValuation(down, valuation(3, '99.75'), 40).move).toBe('up')
  })
  it('judge a row by the server: status, quality and the stamped validity', () => {
    const now = Date.parse('2026-11-22T12:00:00Z')
    expect(rowState(null, now)).toBe('awaiting')
    expect(rowState(valuation(1, '99'), now)).toBe('valid')
    expect(rowState(valuation(1, '99', { status: 'Partial' }), now)).toBe('partial')
    expect(rowState(valuation(1, '99', { status: 'Failed' }), now)).toBe('failed')
    expect(rowState(valuation(1, '99', { quality: 'Stale' }), now)).toBe('stale')
    expect(rowState(valuation(1, '99', { validUntil: '2026-11-22T11:59:59Z' }), now)).toBe('expired')
  })
})

describe('rows', () => {
  const rules: PricingRule[] = [
    { instrument_id: 'BOND-002', method: { kind: 'yield' } },
    { instrument_id: 'BOND-001', method: { kind: 'benchmark_spread', tenor: '10Y' }, benchmark_set_id: 'UST-OTR' },
  ]
  it('list each rule once, in instrument order, with the desk marks where the desk quotes it', () => {
    const desk = {
      source: 'desk',
      levels: [{ instrument_id: 'BOND-001', field: 'mid_spread' as const, mark: { level: '125', marked_at: '2026-11-22T12:00:00Z' } }],
    }
    const rows = sheetRows(rules, [corporate], desk)
    expect(rows.map((row) => row.instrumentId)).toEqual(['BOND-001', 'BOND-002'])
    expect(rows[0]!.deskField).toBe('mid_spread')
    expect(shownLevel(rows[0]!)).toEqual({ value: '125', field: 'mid_spread' })
    expect(rows[1]!.bond).toBeNull()
    expect(rows[1]!.deskField).toBeNull()
  })
  it('show a level the desk does not mark from the measure that equals it', () => {
    const [, yieldRow] = sheetRows(rules, [], null)
    expect(shownLevel(yieldRow!)).toBeNull()
    expect(shownLevel(withValuation(yieldRow!, valuation(1, '99'), 0))).toEqual({ value: 0.0525, field: 'mid_yield' })
  })
})

describe('stream', () => {
  it('offers its token base64url-encoded beside the stream protocol, and only then', () => {
    expect(streamProtocols(null)).toEqual(['convex.v1'])
    const [stream, bearer] = streamProtocols('a+b/c==')
    expect(stream).toBe('convex.v1')
    expect(bearer).toBe(`convex.bearer.${btoa('a+b/c==').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')}`)
    expect(bearer).toMatch(/^convex\.bearer\.[A-Za-z0-9_-]+$/)
  })
  it('subscribes at most 256 instruments a message', () => {
    const ids = Array.from({ length: 600 }, (_, index) => `B${index}`)
    const messages = subscriptionMessages(ids).map((text) => JSON.parse(text) as { instrument_ids: string[] })
    expect(messages.map((message) => message.instrument_ids.length)).toEqual([256, 256, 88])
    expect(messages.flatMap((message) => message.instrument_ids)).toEqual(ids)
  })
})
