import { describe, expect, it } from 'vitest'
import { bookSubscription, closeTaken, holdLater, MAX_SUMMARY_BOOKS, noBookPnlText, summariesOn, withSummaries, type BookSummary } from './books'
import { bookList } from './followedBooks'
import { boundsLabel, countStatuses, countsSummary, formatLimitValue, measureLabel, unknownReasonText, worstCounted } from './limits'

const STARTED = '2026-11-27T09:00:00Z'

function summary(bookId: string, sequence: number, overrides: Partial<BookSummary> = {}): BookSummary {
  return {
    book_id: bookId,
    book_version: 1,
    limit_set_version: 1,
    sequence,
    monitor_started_at: STARTED,
    evaluated_at: '2026-11-27T12:00:00Z',
    currency: 'USD',
    settlement_date: '2026-11-30',
    risk: { status: 'unknown', reason: { code: 'book_currency_required' } },
    pnl: { status: 'unknown', reason: 'no_close' },
    limits: [],
    ...overrides,
  }
}

const merged = (...arrived: BookSummary[]) => withSummaries(new Map(), arrived)

describe('book summaries', () => {
  it('keep the higher sequence of one monitor, whatever order they arrive in', () => {
    const later = merged(summary('BOOK-IG', 7))
    const kept = withSummaries(later, [summary('BOOK-IG', 5)])
    expect(kept.get('BOOK-IG')?.sequence).toBe(7)
    expect(kept).toBe(later)
    expect(withSummaries(kept, [summary('BOOK-IG', 8)]).get('BOOK-IG')?.sequence).toBe(8)
    expect(merged(summary('BOOK-IG', 9), summary('BOOK-IG', 4)).get('BOOK-IG')?.sequence).toBe(9)
  })

  it('take a later start of the monitor over a higher sequence of an earlier one', () => {
    const restarted = withSummaries(merged(summary('BOOK-IG', 900)), [
      summary('BOOK-IG', 1, { monitor_started_at: '2026-11-27T10:00:00Z' }),
    ])
    expect(restarted.get('BOOK-IG')?.sequence).toBe(1)
    expect(withSummaries(restarted, [summary('BOOK-IG', 901)]).get('BOOK-IG')?.sequence).toBe(1)
  })

  it('are held at most one a book until merged', () => {
    const held = new Map<string, BookSummary>()
    for (const sequence of [3, 9, 4]) holdLater(held, summary('BOOK-IG', sequence))
    holdLater(held, summary('BOOK-HY', 2))
    expect([...held.values()].map((one) => [one.book_id, one.sequence])).toEqual([
      ['BOOK-IG', 9],
      ['BOOK-HY', 2],
    ])
  })

  it('are kept by book, and dropped for a book no longer followed', () => {
    const both = merged(summary('BOOK-IG', 1), summary('BOOK-HY', 1))
    expect([...both.keys()]).toEqual(['BOOK-IG', 'BOOK-HY'])
    expect([...summariesOn(both, ['BOOK-HY']).keys()]).toEqual(['BOOK-HY'])
  })

  it('count their limits by status, the worst first', () => {
    const counted = countStatuses(['within', 'unknown', 'breach', 'within'])
    expect(worstCounted(counted)).toBe('breach')
    expect(countsSummary(counted)).toBe('breach 1 · unknown 1 · within 2')
    expect(countsSummary({})).toBe('no limits')
    expect(worstCounted({})).toBeNull()
  })
})

describe('the book topic', () => {
  it('subscribes to at most a session of books', () => {
    expect(JSON.parse(bookSubscription(['BOOK-IG', 'BOOK-HY']))).toEqual({
      type: 'subscribe_books',
      book_ids: ['BOOK-IG', 'BOOK-HY'],
    })
    const many = Array.from({ length: MAX_SUMMARY_BOOKS + 1 }, (_, index) => `BOOK-${index}`)
    expect(() => bookSubscription(many)).toThrow(/at most 64/)
  })

})

describe('the books followed', () => {
  it('are each book typed, once, in order', () => {
    expect(bookList(' BOOK-IG, BOOK-HY BOOK-IG,,')).toEqual(['BOOK-IG', 'BOOK-HY'])
  })
})

describe('limits as the desk words them', () => {
  it('name each measure and its member', () => {
    expect(measureLabel({ kind: 'face', instrument: 'each' })).toBe('Face · each')
    expect(measureLabel({ kind: 'face', instrument: { one: 'IG-1' } })).toBe('Face · IG-1')
    expect(measureLabel({ kind: 'key_rate_dv01', tenor: '10Y' })).toBe('Key-rate DV01 10Y')
    expect(measureLabel({ kind: 'group_share', by: 'issuer', group: 'each' })).toBe('issuer share · each')
    expect(measureLabel({ kind: 'dv01' })).toBe('DV01')
  })

  it('show bounds and values in the measure unit', () => {
    const share = { kind: 'group_share', by: 'sector', group: { one: 'Energy' } } as const
    expect(boundsLabel({ measure: share, upper: '0.25' })).toBe('≤ 25.0%')
    expect(boundsLabel({ measure: { kind: 'dv01' }, lower: '-50000', upper: '50000' })).toBe('-50,000 to 50,000')
    expect(boundsLabel({ measure: { kind: 'market_value' }, lower: '0' })).toBe('≥ 0')
    expect(formatLimitValue({ kind: 'modified_duration' }, '4.4567')).toBe('4.46')
    expect(formatLimitValue({ kind: 'market_value' }, '49870123.456')).toBe('49,870,123')
  })

  it('say why a value is unknown, naming the first positions', () => {
    expect(unknownReasonText({ code: 'missing_result', instruments: { count: 3, first: ['IG-1', 'IG-2'] } })).toBe(
      'missing result: 3 positions (IG-1, IG-2, …)',
    )
    expect(
      unknownReasonText({ code: 'measure_not_computed', measure: 'dv01_mid', instruments: { count: 1, first: ['HY-1'] } }),
    ).toBe('measure not computed: dv01_mid: 1 position (HY-1)')
    expect(unknownReasonText({ code: 'quote_refused', instrument: 'IG-3', detail: 'stale quote' })).toBe(
      'quote refused: IG-3: stale quote',
    )
    expect(noBookPnlText('no_close')).toBe('no close yet')
    expect(noBookPnlText('close_settles_later')).toBe('close settles later')
  })

  it('report a close taken', () => {
    const close = {
      version: 2,
      close_date: '2026-11-27',
      positions: [{ value: { status: 'valued' as const } }, { value: { status: 'unvalued' as const } }, { value: { status: 'valued' as const } }],
    }
    expect(closeTaken('BOOK-IG', close)).toBe('BOOK-IG closed for 2026-11-27 (version 2): 2 valued, 1 unvalued')
  })
})
