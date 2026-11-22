// Wire shapes both apps read: the Live runtime's published results, as
// `GET /api/v1/runtime/results` pages them and the stream sends them, a page of stored
// bonds, and a page of master resources. Only the fields the screens read are named.

export interface Measure {
  value: { value: number | string; unit: string }
  quality: string
  method: string
}

/** The field of its quote a result was valued from, as observed: its value is exact. */
export interface QuoteInput {
  field: string
  observation: {
    value: { value: string; unit: string }
    source_time_ms: number
    receive_time_ms: number
    sequence: number
  }
}

export interface PublishedValuation {
  runtime_id: string
  revision: number
  generation: number
  live_valid_until: string | null
  result: {
    request_id: string
    status: string
    quality: string
    configuration_version: number
    results: Record<string, Measure>
    errors: unknown[]
    provenance: { subject: { instrument_id?: string; version?: number } }
  }
  quote_input?: QuoteInput
}

/** One page of `GET /api/v1/runtime/results`, in instrument ID order. */
export interface RuntimeResultsPage {
  results: PublishedValuation[]
  next_after: string | null
  more: boolean
}

export interface BondPage {
  bonds: Record<string, unknown>[]
  versions: { instrument_id: string; version: number }[]
  total: number
  offset: number
  limit: number
}

/**
 * Every runtime result, read a page at a time from the start. The cursor only
 * moves forward, so each instrument is on one page of the walk and the pages join
 * without duplicates.
 */
export async function readAllPages(
  read: (after: string | null) => Promise<RuntimeResultsPage>,
): Promise<PublishedValuation[]> {
  const results: PublishedValuation[] = []
  let after: string | null = null
  for (;;) {
    const page = await read(after)
    results.push(...page.results)
    if (!page.more || page.next_after === null) return results
    after = page.next_after
  }
}

/** A master resource as `GET /api/v1/reference/masters/:kind` lists it. */
export interface MasterRecord<Definition = Record<string, unknown>> {
  version: number
  recorded_at: string
  definition: { kind: string; definition: Definition }
}

/** One page of `GET /api/v1/reference/masters/:kind`, resumed after `next`. */
export interface MasterPage<Definition = Record<string, unknown>> {
  items: MasterRecord<Definition>[]
  next: string | null
}
