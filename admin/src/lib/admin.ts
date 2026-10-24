import { parse as parseExact, stringify as stringifyExact } from 'lossless-json'

// Pure helpers shared by the screens. Nothing here validates domain data: the
// server's strict decoders are the only validators (D40).
//
// The server compares numbers with their written scale, so `1.0` and `1` differ.
// JSON.parse/JSON.stringify would rewrite one as the other: anything sent back is
// either the operator's own text or re-rendered here without touching numbers.

/** Parse a response keeping every number exactly as written. */
export function readExact(text: string): unknown {
  return parseExact(text)
}

/** Render a value read with `readExact`, numbers unchanged, indented for an editor. */
export function writeExact(value: unknown): string {
  return stringifyExact(value, null, 2) ?? ''
}

/**
 * A JSON object whose member values are JSON texts used verbatim, so a document or
 * fixture pasted by the operator reaches the server byte for byte.
 */
export function rawObject(members: Record<string, string>): string {
  return `{${Object.entries(members)
    .map(([name, text]) => `${JSON.stringify(name)}:${text}`)
    .join(',')}}`
}

/** One importer finding: where a refused file went wrong, never its values. */
export interface Diagnostic {
  section: string
  row: number | null
  field: string
  code: string
  line: number | null
  column: number | null
}

/** A refused request, decoded from any of the server's error bodies. */
export class ApiFailure extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    readonly reason: string,
    readonly diagnostics: Diagnostic[] = [],
  ) {
    super(`${status} ${code}: ${reason}`)
  }
}

type Json = Record<string, unknown>

function isObject(value: unknown): value is Json {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * The server answers refusals in three forms: configuration routes send
 * `{error: {code, reason}}`, reference and access refusals send `{code, error}`
 * (reference ones may add `diagnostics`), and framework rejections send text.
 */
export function decodeFailure(status: number, body: unknown, text: string): ApiFailure {
  if (isObject(body)) {
    const error = body.error
    if (isObject(error)) {
      return new ApiFailure(status, String(error.code ?? status), String(error.reason ?? ''))
    }
    const diagnostics = Array.isArray(body.diagnostics) ? (body.diagnostics as Diagnostic[]) : []
    return new ApiFailure(
      status,
      String(body.code ?? status),
      String(error ?? body.message ?? ''),
      diagnostics,
    )
  }
  return new ApiFailure(status, String(status), text.trim().slice(0, 500))
}

/** A fresh idempotency key; the server accepts up to 128 of `[A-Za-z0-9_.:/-]`. */
export function newIdempotencyKey(purpose: string): string {
  return `admin-${purpose}-${crypto.randomUUID()}`
}

/** A reference record's version from its quoted ETag, as `GET /api/v1/bonds/:id` sends it. */
export function etagVersion(etag: string | null): number | null {
  const match = etag?.match(/^"([1-9][0-9]*)"$/)
  return match?.[1] ? Number(match[1]) : null
}

/** The runtime fields that say which activation the running work accepted. */
export interface RuntimeSelection {
  activation_revision: number | null
  desired_activation_revision: number | null
}

export type RestartState = 'unknown' | 'current' | 'applying' | 'pending_restart'

/**
 * Compare the configuration's active revision with what the runtime accepted. A
 * restart-required activation commits while the runtime runs and applies at the
 * next start (D39), so a mismatch the runtime is not applying is a pending restart.
 */
export function restartState(health: RuntimeSelection, active: number | null): RestartState {
  if (active === null) return 'unknown'
  if (health.activation_revision === active) return 'current'
  if (health.desired_activation_revision === active) return 'applying'
  return 'pending_restart'
}

/** A master resource as `GET /api/v1/reference/masters/:kind` lists it. */
export interface MasterRecord {
  version: number
  recorded_at: string
  definition: { kind: string; definition: Json }
}

/** A complete replacement of one master resource at its current version. */
export function masterEditCommand(record: MasterRecord, key: string): Json {
  return {
    idempotency_key: key,
    changes: [],
    master_changes: [{ expected_version: record.version, definition: record.definition }],
  }
}

/** A complete replacement of one bond at its current version. */
export function bondEditCommand(bond: Json, version: number, key: string): Json {
  return { idempotency_key: key, changes: [{ kind: 'update', expected_version: version, bond }] }
}

/** A master resource's own identity field, such as `calendar_id` or `mapping_id`. */
export function masterLabel(record: MasterRecord): string {
  const definition = record.definition.definition
  const field = Object.keys(definition).find((name) => name.endsWith('_id'))
  return field ? String(definition[field]) : record.definition.kind
}

/** Headers for a configuration write: always a key, and If-Match where the route needs it. */
export function configurationWriteHeaders(key: string, expectedRevision?: number): Record<string, string> {
  const headers: Record<string, string> = { 'Idempotency-Key': key }
  if (expectedRevision !== undefined) headers['If-Match'] = String(expectedRevision)
  return headers
}

/** The uploaded import is refused above this size, as the server does. */
export const MAX_IMPORT_BYTES = 8 * 1024 * 1024

/** A published measure, `{value, unit}`. Decimal strings are shown exactly as sent. */
export function formatMeasure(measure: { value: number | string; unit: string }): string {
  const value =
    typeof measure.value === 'number' ? String(Number(measure.value.toPrecision(8))) : measure.value
  return `${value} ${measure.unit}`
}
