import { bench, describe } from 'vitest'
import { blotterOrder, MAX_FINAL_ROWS, withRows, type RfqRow } from './rfqs'

// The blotter's merge and order at its bounds: a seed of the desk's 256 open requests, and
// one transition into a blotter holding them and the 100 final requests it keeps.
// `npx vitest bench --run`.

const OPEN = 256

function row(index: number, recordId: number, state: RfqRow['state']): RfqRow {
  return {
    rfq_key: index.toString(16).padStart(64, '0'),
    record_id: recordId,
    state,
    request: {
      source_id: 'venue-sim',
      rfq_id: `RFQ-${index}`,
      client_id: `CLIENT-${index % 24}`,
      client_tier: '2',
      instrument_id: `BOND-${index % 40}`,
      side: index % 2 === 0 ? 'buy' : 'sell',
      face: '5000000',
      settlement_date: '2026-11-25',
      book_id: `BOOK-${index % 4}`,
      deadline: new Date(Date.parse('2026-11-24T12:00:00Z') + ((index * 7919) % 300_000)).toISOString(),
    },
    instrument_id: `BOND-${index % 40}`,
    instrument_version: 1,
    received_at: '2026-11-24T11:59:00Z',
    auto_price: {
      kind: 'priced',
      desk_side: 'bid',
      clean_price: '99.24373766953476661001332101',
      level: { value: '127', unit: 'spread_basis_points' },
      clean_price_mid: '99.27',
      mid_only: null,
    },
    check: { kind: 'evaluated', statuses: { within: 2 } },
    quote: null,
  }
}

const seed = Array.from({ length: OPEN }, (_, index) => row(index, index + 1, 'priced'))
const finals = Array.from({ length: MAX_FINAL_ROWS }, (_, index) => row(OPEN + index, OPEN + index + 1, 'done'))
const full = withRows(new Map(), [...seed, ...finals])
let next = OPEN + MAX_FINAL_ROWS + 1

describe('the blotter at its bounds', () => {
  bench('a seed of 256 open requests, merged and ordered', () => {
    blotterOrder(withRows(new Map(), seed))
  })
  bench('one transition into 256 open and 100 final, merged and ordered', () => {
    next += 1
    blotterOrder(withRows(full, [row(next % OPEN, next, 'quoted')]))
  })
})
