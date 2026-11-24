import { describe, expect, it } from 'vitest'
import { ApiFailure, callQueue, decodeFailure, rawObject, readAllPages } from './index'

describe('readAllPages', () => {
  const record = (id: string, revision: number) =>
    ({ runtime_id: 'R', revision, result: { provenance: { subject: { instrument_id: id } } } }) as never

  it('walks from the start until no more follow, resuming after each page', async () => {
    const asked: (string | null)[] = []
    const pages = new Map([
      [null, { results: [record('A', 3), record('B', 1)], next_after: 'B', more: true }],
      ['B', { results: [record('C', 2)], next_after: null, more: false }],
    ])
    const all = await readAllPages(async (after) => {
      asked.push(after)
      return pages.get(after as never)!
    })
    expect(asked).toEqual([null, 'B'])
    expect(all.map((r) => (r as { revision: number }).revision)).toEqual([3, 1, 2])
  })

  it('reads one page of a runtime with no results', async () => {
    let reads = 0
    const all = await readAllPages(async () => {
      reads += 1
      return { results: [], next_after: null, more: false }
    })
    expect(all).toEqual([])
    expect(reads).toBe(1)
  })
})

describe('decodeFailure', () => {
  it('reads configuration refusals', () => {
    const failure = decodeFailure(409, { schema_version: 1, error: { code: 'CONFIGURATION_CONFLICT', reason: 'stale' } }, '')
    expect([failure.status, failure.code, failure.reason]).toEqual([409, 'CONFIGURATION_CONFLICT', 'stale'])
  })
  it('reads reference refusals with importer diagnostics', () => {
    const diagnostic = { section: 'changes', row: 0, field: 'day_count', code: 'INVALID_INPUT', line: null, column: null }
    const failure = decodeFailure(400, { code: 'INVALID_INPUT', error: 'bad row', diagnostics: [diagnostic] }, '')
    expect(failure.code).toBe('INVALID_INPUT')
    expect(failure.diagnostics).toEqual([diagnostic])
  })
  it('reads the rows a quote batch refused', () => {
    const row = { index: 0, code: 'LEVEL_OUT_OF_RANGE', reason: 'the level is outside the rule range' }
    const failure = decodeFailure(422, { code: 'QUOTES_REJECTED', error: 'batch rejected; nothing was queued', rows: [row], truncated: false }, '')
    expect([failure.code, failure.rows]).toEqual(['QUOTES_REJECTED', [row]])
  })
  it('reads access refusals and plain text', () => {
    expect(decodeFailure(403, { code: 'FORBIDDEN', error: 'FORBIDDEN' }, '').reason).toBe('FORBIDDEN')
    const text = decodeFailure(400, null, 'Failed to deserialize query string')
    expect(text).toBeInstanceOf(ApiFailure)
    expect(text.reason).toBe('Failed to deserialize query string')
  })
})

describe('exact JSON', () => {
  it('embeds pasted documents verbatim', () => {
    const document = '{"value": 1.0}'
    expect(rawObject({ document, reason: JSON.stringify('a "quoted" reason') })).toBe(
      '{"document":{"value": 1.0},"reason":"a \\"quoted\\" reason"}',
    )
  })
})

describe('callQueue', () => {
  it('runs at most its limit at once, the rest in the order asked, a failure included', async () => {
    const queued = callQueue(2)
    let running = 0
    let most = 0
    const order: number[] = []
    const run = (index: number) =>
      queued(async () => {
        running += 1
        most = Math.max(most, running)
        await new Promise((resolve) => setTimeout(resolve, 5))
        running -= 1
        order.push(index)
        if (index === 1) throw new Error('refused')
        return index
      })
    const settled = await Promise.allSettled([0, 1, 2, 3, 4].map(run))
    expect(most).toBe(2)
    expect(order).toEqual([0, 1, 2, 3, 4])
    expect(settled.map((result) => result.status)).toEqual(['fulfilled', 'rejected', 'fulfilled', 'fulfilled', 'fulfilled'])
  })
})
