import { describe, expect, it } from 'vitest'
import {
  ApiFailure,
  bondEditCommand,
  configurationWriteHeaders,
  decodeFailure,
  etagVersion,
  formatMeasure,
  masterEditCommand,
  masterLabel,
  newIdempotencyKey,
  rawObject,
  readExact,
  restartState,
  writeExact,
  type MasterRecord,
} from './admin'

const calendar: MasterRecord = {
  version: 3,
  recorded_at: '2026-10-07T12:00:00Z',
  definition: { kind: 'calendar', definition: { calendar_id: 'calendar-example', code: 'weekends_only' } },
}

describe('restartState', () => {
  it('reports a restart-required activation the runtime has not accepted', () => {
    expect(restartState({ activation_revision: 3, desired_activation_revision: 3 }, 4)).toBe('pending_restart')
  })
  it('reports current, applying and unknown', () => {
    expect(restartState({ activation_revision: 4, desired_activation_revision: 4 }, 4)).toBe('current')
    expect(restartState({ activation_revision: 3, desired_activation_revision: 4 }, 4)).toBe('applying')
    expect(restartState({ activation_revision: null, desired_activation_revision: null }, null)).toBe('unknown')
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
  it('reads access refusals and plain text', () => {
    expect(decodeFailure(403, { code: 'FORBIDDEN', error: 'FORBIDDEN' }, '').reason).toBe('FORBIDDEN')
    const text = decodeFailure(400, null, 'Failed to deserialize query string')
    expect(text).toBeInstanceOf(ApiFailure)
    expect(text.reason).toBe('Failed to deserialize query string')
  })
})

describe('edit commands', () => {
  it('replace a master resource whole at its current version', () => {
    expect(masterEditCommand(calendar, 'k')).toEqual({
      idempotency_key: 'k',
      changes: [],
      master_changes: [{ expected_version: 3, definition: calendar.definition }],
    })
    expect(masterLabel(calendar)).toBe('calendar-example')
  })
  it('update a bond at the version its ETag names', () => {
    expect(etagVersion('"7"')).toBe(7)
    expect(etagVersion('7')).toBeNull()
    expect(etagVersion('"0"')).toBeNull()
    expect(etagVersion(null)).toBeNull()
    expect(bondEditCommand({ instrument_id: 'X' }, 7, 'k')).toEqual({
      idempotency_key: 'k',
      changes: [{ kind: 'update', expected_version: 7, bond: { instrument_id: 'X' } }],
    })
  })
})

describe('exact JSON', () => {
  it('keeps the written scale of every number', () => {
    const text = '{"version":3,"definition":{"kind":"curve","definition":{"quote":1.0,"rate":0.037,"big":1e21}}}'
    expect(JSON.stringify(JSON.parse(text))).not.toContain('1.0')
    const command = masterEditCommand(readExact(text) as MasterRecord, 'k')
    const written = writeExact(command)
    expect(written).toContain('"quote": 1.0')
    expect(written).toContain('"rate": 0.037')
    expect(written).toContain('"big": 1e21')
    expect(written).toContain('"expected_version": 3')
  })
  it('embeds pasted documents verbatim', () => {
    const document = '{"value": 1.0}'
    expect(rawObject({ document, reason: JSON.stringify('a "quoted" reason') })).toBe(
      '{"document":{"value": 1.0},"reason":"a \\"quoted\\" reason"}',
    )
  })
})

describe('headers and keys', () => {
  it('send If-Match only when the write names an expected revision', () => {
    expect(configurationWriteHeaders('k')).toEqual({ 'Idempotency-Key': 'k' })
    expect(configurationWriteHeaders('k', 0)).toEqual({ 'Idempotency-Key': 'k', 'If-Match': '0' })
  })
  it('make keys the server accepts', () => {
    const key = newIdempotencyKey('stage')
    expect(key).toMatch(/^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,127}$/)
    expect(newIdempotencyKey('stage')).not.toBe(key)
  })
  it('show decimal strings exactly and round floats for display', () => {
    expect(formatMeasure({ value: '99.5413', unit: 'price_per_100' })).toBe('99.5413 price_per_100')
    expect(formatMeasure({ value: 0.055291869588800675, unit: 'decimal_rate' })).toBe('0.05529187 decimal_rate')
  })
})
