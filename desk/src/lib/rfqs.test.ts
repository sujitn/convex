import { ApiFailure } from '@convex/api-client'
import { describe, expect, it, vi } from 'vitest'
import { roundToTick } from './decimal'
import { bookList } from './followedBooks'
import { formatQuoteValue } from './format'
import {
  blotterOrder,
  checkSummary,
  formatCountdown,
  formatFace,
  isAnswerable,
  MAX_FINAL_ROWS,
  nudgedPrice,
  rowFromRecords,
  secondsLeft,
  sentPrice,
  tickedPrice,
  TREASURY_TICK,
  unanswerable,
  quotingTick,
  withRows,
  worstStatus,
  type AutoPrice,
  type RfqRecord,
  type RfqRow,
  type RfqState,
} from './rfqs'
import { MAX_RFQ_BOOKS, RfqTopicReader, rfqSubscription, type RfqTopicHandlers } from './rfqTopic'
import type { StreamMessage } from './stream'

const NOW = Date.parse('2026-11-24T12:00:00Z')
const PRICE = '0.001'

function row(rfqKey: string, recordId: number, state: RfqState, overrides: Partial<RfqRow> = {}): RfqRow {
  return {
    rfq_key: rfqKey,
    record_id: recordId,
    state,
    request: {
      source_id: 'venue-sim',
      rfq_id: `RFQ-${rfqKey}`,
      client_id: 'CLIENT-17',
      client_tier: '2',
      instrument_id: 'BOND-001',
      side: 'sell',
      face: '5000000',
      settlement_date: '2026-11-25',
      book_id: 'BOOK-IG',
      deadline: '2026-11-24T12:00:30Z',
    },
    instrument_id: 'BOND-001',
    instrument_version: 1,
    received_at: '2026-11-24T11:59:30Z',
    auto_price: twoWay('bid', '99.24373766953476661001332101'),
    check: { kind: 'evaluated', statuses: { within: 2 } },
    quote: null,
    ...overrides,
  }
}

function twoWay(side: 'bid' | 'ask', cleanPrice: string, midOnly: string | null = null): AutoPrice {
  return {
    kind: 'priced',
    desk_side: side,
    clean_price: cleanPrice,
    level: { value: '127', unit: 'spread_basis_points' },
    clean_price_mid: '99.27',
    mid_only: midOnly,
  }
}

function deadline(at: string): Partial<RfqRow> {
  return { request: { ...row('x', 1, 'priced').request, deadline: at } }
}

/** A request's entries: received, priced, quoted at 99.243, then `state` at `recordId`. */
function entries(recordId: number, state: 'done' | 'expired'): RfqRecord[] {
  const head = { actor: 'server' as const, user: null, recorded_at: '' }
  const request = row('x', 1, 'priced').request
  return [
    { ...head, record_id: 1, state: 'received', request, instrument_id: 'BOND-001', instrument_version: 1, received_at: '' },
    {
      ...head,
      record_id: 2,
      state: 'priced',
      configuration: { configuration_id: 'C', version: 1, activation_revision: 1, client_tier: '2', width_factor: '1' },
      auto_price: { kind: 'unpriced', reason: 'no rule' },
      check: { kind: 'unavailable', reason: 'the book has no limits' },
      priced_at: '',
    },
    { ...head, actor: 'trader', record_id: 7, state: 'quoted', clean_price: '99.243', quoted_at: '2026-11-24T12:00:01Z' },
    state === 'expired' ? { ...head, record_id: recordId, state, reason: 'deadline' } : { ...head, actor: 'source', record_id: recordId, state },
  ]
}

describe('a sent price', () => {
  it('rounds exactly to the tick, down or up, written without zeros past three places', () => {
    expect(roundToTick('99.24373766953476661001332101', '0.001', 'down')).toBe('99.243')
    expect(roundToTick('99.24373766953476661001332101', '0.001', 'up')).toBe('99.244')
    expect(roundToTick('99.243', '0.001', 'up')).toBe('99.243')
    expect(roundToTick('99.2437', '0.00390625', 'up')).toBe('99.24609375')
    expect(roundToTick('99.2437', '0.00390625', 'down')).toBe('99.2421875')
    expect(roundToTick('99.5', '0.00390625', 'down')).toBe('99.500')
    expect(roundToTick('100', '0.001', 'up')).toBe('100.000')
    expect(roundToTick('-0.0015', '0.001', 'down')).toBe('-0.002')
    // A whole tick keeps a whole number's zeros.
    expect(roundToTick('100', '1', 'up')).toBe('100')
    expect(roundToTick('0', '1', 'down')).toBe('0')
    expect(roundToTick('100.5', '1', 'up')).toBe('101.0')
    expect(roundToTick('99.37', '0.25', 'down')).toBe('99.25')
  })
  it('is the auto price on the tick in the desk’s favour: a bid down, an ask up', () => {
    expect(sentPrice(twoWay('bid', '99.2437'), PRICE)).toBe('99.243')
    expect(sentPrice(twoWay('ask', '99.2431'), PRICE)).toBe('99.244')
    expect(sentPrice(twoWay('ask', '99.2431'), TREASURY_TICK)).toBe('99.24609375')
  })
  it('is never a mid or an unpriced request on one key, though an adjust starts from the mid', () => {
    const mid = twoWay('ask', '99.2431', 'the bond’s rule sets no two-way width')
    expect(sentPrice(mid, PRICE)).toBeNull()
    expect(tickedPrice(mid, PRICE)).toBe('99.244')
    const unpriced: AutoPrice = { kind: 'unpriced', reason: 'no rule' }
    expect(sentPrice(unpriced, PRICE)).toBeNull()
    expect(tickedPrice(unpriced, PRICE)).toBeNull()
  })
  it('is quoted on a Treasury’s 256th or a thousandth, and on no tick while the bond is unknown', () => {
    const corporate = { issuer_type: 'Corporate', currency: 'USD', bond_type: 'FixedBullet', cusip: '000000AA0' } as never
    const treasury = { issuer_type: 'Sovereign', currency: 'USD', bond_type: 'FixedBullet', cusip: '91282CBB6' } as never
    expect([quotingTick(corporate), quotingTick(treasury), quotingTick(null), quotingTick(undefined)]).toEqual([
      PRICE,
      TREASURY_TICK,
      null,
      null,
    ])
  })
  it('moves a tick at a time, onto the tick from a typed price off it', () => {
    expect(nudgedPrice('99.243', PRICE, 1)).toBe('99.244')
    expect(nudgedPrice('99.243', PRICE, -1)).toBe('99.242')
    expect(nudgedPrice('99.2435', PRICE, 1)).toBe('99.244')
    expect(nudgedPrice('99.2435', PRICE, -1)).toBe('99.243')
    expect(nudgedPrice('99.5', TREASURY_TICK, 1)).toBe('99.50390625')
  })
})

describe('the blotter’s rows', () => {
  it('keep the later row of each request by record id', () => {
    const first = withRows(new Map(), [row('a', 5, 'priced'), row('b', 6, 'priced')])
    const quoted = withRows(first, [row('a', 9, 'quoted')])
    expect(quoted.get('a')?.state).toBe('quoted')
    // A seed's row older than a transition already merged changes nothing.
    expect(withRows(quoted, [row('a', 5, 'priced')])).toBe(quoted)
    expect(withRows(quoted, [row('a', 9, 'quoted')])).toBe(quoted)
  })
  it('keep every open request and only the latest final ones', () => {
    const finals = Array.from({ length: MAX_FINAL_ROWS + 5 }, (_, index) => row(`f${index}`, index + 1, 'done'))
    const rows = withRows(new Map(), [row('open', 1, 'priced'), ...finals])
    expect(rows.size).toBe(MAX_FINAL_ROWS + 1)
    expect(rows.has('open')).toBe(true)
    expect(rows.has('f4')).toBe(false)
    expect(rows.has('f5')).toBe(true)
  })
  it('list the open requests by deadline, then the final ones latest first', () => {
    const rows = withRows(new Map(), [
      row('late', 1, 'priced', deadline('2026-11-24T12:04:00Z')),
      row('done-old', 2, 'done'),
      row('soon', 3, 'quoted', deadline('2026-11-24T12:00:05Z')),
      row('done-new', 4, 'traded_away'),
    ])
    expect(blotterOrder(rows).map((shown) => shown.rfq_key)).toEqual(['soon', 'late', 'done-new', 'done-old'])
  })
  it('take a request read again as its last entry left it', () => {
    const read = rowFromRecords(row('a', 2, 'priced'), entries(9, 'done'))
    expect([read.state, read.record_id, read.quote?.clean_price]).toEqual(['done', 9, '99.243'])
  })
})

describe('answering a request', () => {
  it('is open to a priced request before its deadline, once', () => {
    const priced = row('a', 1, 'priced')
    expect(isAnswerable(priced, NOW, false)).toBe(true)
    expect(isAnswerable(priced, NOW, true)).toBe(false)
    expect(isAnswerable(priced, Date.parse('2026-11-24T12:00:30Z'), false)).toBe(false)
    expect(isAnswerable(row('a', 2, 'quoted'), NOW, false)).toBe(false)
    expect(isAnswerable(row('a', 3, 'expired'), NOW, false)).toBe(false)
  })
  it('says why nothing was sent', () => {
    expect(unanswerable(undefined)).toBe('the request is no longer on the blotter')
    expect(unanswerable(row('a', 2, 'traded_away'))).toBe('the request is traded away')
    expect(unanswerable(row('a', 1, 'priced'))).toBe('the request is past its deadline')
  })
})

describe('the blotter’s cells', () => {
  it('count down on the server’s clock', () => {
    expect(secondsLeft('2026-11-24T12:00:30Z', NOW)).toBe(30)
    expect(secondsLeft('2026-11-24T12:00:30Z', NOW + 29_500)).toBe(0)
    expect(secondsLeft('2026-11-24T12:00:30Z', NOW + 60_000)).toBe(0)
    expect([formatCountdown(42), formatCountdown(245)]).toEqual(['42s', '4:05'])
  })
  it('size a trade in thousands or millions', () => {
    expect(['100000', '750000', '1000000', '2500000', '25000000'].map(formatFace)).toEqual([
      '100K',
      '750K',
      '1MM',
      '2.5MM',
      '25MM',
    ])
  })
  it('show a quote’s value in its unit', () => {
    expect(formatQuoteValue({ value: '127', unit: 'spread_basis_points' })).toBe('127.0')
    expect(formatQuoteValue({ value: '126.25', unit: 'spread_basis_points' })).toBe('126.25')
    expect(formatQuoteValue({ value: '0.0525', unit: 'yield_decimal' })).toBe('5.250')
    expect(formatQuoteValue({ value: '99.5', unit: 'price_per100' }, true)).toBe('99-16')
  })
  it('show the check’s most severe status first', () => {
    const check = { kind: 'evaluated' as const, statuses: { within: 1, breach: 1, warning: 2 } }
    expect(worstStatus(check)).toBe('breach')
    expect(checkSummary(check)).toBe('breach 1 · warning 2 · within 1')
    expect(worstStatus({ kind: 'unavailable', reason: 'the book has no limits' })).toBeNull()
    expect(checkSummary({ kind: 'evaluated', statuses: {} })).toBe('no limits')
  })
  it('read a typed list of books once each', () => {
    expect(bookList(' BOOK-IG, BOOK-HY BOOK-IG,,')).toEqual(['BOOK-IG', 'BOOK-HY'])
  })
})

describe('the rfq topic', () => {
  function reader(held: RfqRow[] = [], records: (rfqKey: string) => Promise<RfqRecord[]> = async () => []) {
    const sent: string[] = []
    const seen = { rows: [] as string[], gone: [] as string[], refusals: [] as string[], failures: 0 }
    const handlers: RfqTopicHandlers = {
      onRow: (shown) => seen.rows.push(`${shown.rfq_key} ${shown.state}`),
      onGone: (rfqKeys) => seen.gone.push(...rfqKeys),
      onWithheld: (bookId) => seen.refusals.push(`withheld ${bookId}`),
      onRefusal: (code) => seen.refusals.push(code),
      onReadFailure: () => (seen.failures += 1),
      onStatus: () => {},
      readRecords: records,
    }
    const topic = new RfqTopicReader(['BOOK-IG'], handlers, held)
    const send = (message: string) => sent.push(message)
    const read = (message: StreamMessage) => topic.read(message, send)
    return { topic, sent, seen, send, read }
  }
  const rfq = (rfqKey: string, recordId = 1, state: RfqState = 'priced'): StreamMessage => ({
    type: 'rfq',
    ...row(rfqKey, recordId, state),
  })
  const subscribed: StreamMessage = { type: 'subscribed', subscription_type: 'rfqs', count: 1 }
  const resync: StreamMessage = { type: 'error', code: 'RESYNC_REQUIRED', message: '' }

  it('covers only the requests named after the last RESYNC_REQUIRED before the acknowledgement', async () => {
    const asked: string[] = []
    const { topic, sent, seen, send, read } = reader([], async (rfqKey) => {
      asked.push(rfqKey)
      return []
    })
    topic.subscribe(send)
    expect(JSON.parse(sent[0]!)).toEqual({ type: 'subscribe_rfqs', book_ids: ['BOOK-IG'] })
    read(rfq('before'))
    read(resync)
    read(rfq('seed'))
    await read(subscribed)
    expect(seen.rows).toEqual(['before priced', 'seed priced'])
    // The request named before the RESYNC_REQUIRED is read again; the seed's is not.
    expect(asked).toEqual(['before'])
    expect(sent).toHaveLength(1)
  })
  it('reads again each request held open, and gives how it ended or that it is gone', async () => {
    const held = [row('ended', 2, 'priced'), row('refused', 3, 'priced'), row('unknown', 4, 'priced'), row('down', 5, 'priced')]
    const { topic, seen, send, read } = reader([...held, row('final', 6, 'done')], async (rfqKey) => {
      if (rfqKey === 'ended') return entries(9, 'expired')
      if (rfqKey === 'refused') throw new ApiFailure(403, 'FORBIDDEN', '')
      if (rfqKey === 'unknown') throw new ApiFailure(404, 'RFQ_UNKNOWN', '')
      throw new ApiFailure(503, 'UNAVAILABLE', '')
    })
    topic.subscribe(send)
    await read(subscribed)
    expect(seen.rows).toEqual(['ended expired'])
    expect(seen.gone.sort()).toEqual(['refused', 'unknown'])
    expect(seen.failures).toBe(1)
  })
  it('subscribes again after a pause when a request could not be read again, until stopped', async () => {
    vi.useFakeTimers()
    try {
      const { topic, sent, seen, send, read } = reader([row('down', 5, 'priced')], async () => {
        throw new ApiFailure(429, 'QUOTA', '')
      })
      topic.subscribe(send)
      await read(subscribed)
      expect(seen.failures).toBe(1)
      await vi.advanceTimersByTimeAsync(5000)
      expect(sent).toHaveLength(2)
      await read(subscribed)
      topic.stop()
      await vi.advanceTimersByTimeAsync(5000)
      expect(sent).toHaveLength(2)
    } finally {
      vi.useRealTimers()
    }
  })
  it('gives nothing from a catch-up still reading once stopped', async () => {
    let finish: (records: RfqRecord[]) => void = () => {}
    const { topic, seen, send, read } = reader(
      [row('ended', 2, 'priced')],
      () => new Promise<RfqRecord[]>((resolve) => (finish = resolve)),
    )
    topic.subscribe(send)
    const caughtUp = read(subscribed)
    topic.stop()
    finish(entries(9, 'expired'))
    await caughtUp
    expect(seen.rows).toEqual([])
  })
  it('gives each request’s later rows only, and nothing after its final row', () => {
    const { seen, read } = reader()
    read(rfq('a', 2))
    read(rfq('a', 2))
    read(rfq('a', 1))
    read(rfq('a', 5, 'done'))
    read(rfq('a', 7, 'done'))
    expect(seen.rows).toEqual(['a priced', 'a done'])
  })
  it('subscribes again on a RESYNC_REQUIRED outside a subscription', async () => {
    const { topic, sent, send, read } = reader()
    topic.subscribe(send)
    await read(subscribed)
    read(rfq('later'))
    read(resync)
    expect(sent).toHaveLength(2)
  })
  it('takes a refused subscription as no catch-up, and counts withheld requests', () => {
    const { topic, seen, send, read } = reader([row('held', 1, 'priced')], async () => {
      throw new Error('not read')
    })
    topic.subscribe(send)
    read({ type: 'error', code: 'FORBIDDEN', message: 'no' })
    expect(read(subscribed)).toBeUndefined()
    expect(read({ type: 'subscribed', subscription_type: 'limits', count: 1 })).toBeUndefined()
    read({ type: 'rfq_withheld', book_id: 'BOOK-IG', code: 'UNAVAILABLE', message: '' })
    expect(seen.refusals).toEqual(['FORBIDDEN', 'withheld BOOK-IG'])
  })
  it('names at most 64 books a subscription', () => {
    const books = Array.from({ length: MAX_RFQ_BOOKS + 1 }, (_, index) => `BOOK-${index}`)
    expect(() => rfqSubscription(books)).toThrow()
    expect(JSON.parse(rfqSubscription(books.slice(0, MAX_RFQ_BOOKS))).book_ids).toHaveLength(MAX_RFQ_BOOKS)
  })
})
