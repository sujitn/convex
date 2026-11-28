import type { BondPage, MasterPage, Reply } from '@convex/api-client'
import { failureStatus } from './failure'
import type { BookPnlDetail } from './books'
import type { LimitEvaluation } from './limits'
import type { RfqRecord } from './rfqs'
import type { Bond, DeskLevels, PricingRule } from './sheet'

// The screens' reads: every pricing rule and bond, a page at a time, the desk's marks, the
// books and one bond, a request for quote's entries, and a book's limits and P&L.

/**
 * Records a page. In required access mode the server checks every bond a reply names
 * against the caller's grants, up to 1,024 names a reply, and a bond page names each bond
 * twice, so a page stays well under that.
 */
const PAGE = 200

type Call = <T>(path: string) => Promise<Reply<T>>

/** Every stored pricing rule, in instrument id order. */
export async function readPricingRules(call: Call): Promise<PricingRule[]> {
  const rules: PricingRule[] = []
  let after: string | null = null
  for (;;) {
    const cursor: string = after === null ? '' : `&after=${encodeURIComponent(after)}`
    const page: MasterPage<PricingRule> = (await call<MasterPage<PricingRule>>(`/api/v1/reference/masters/pricing_rule?limit=${PAGE}${cursor}`)).data
    rules.push(...page.items.map((item) => item.definition.definition))
    if (page.next === null) return rules
    after = page.next
  }
}

/** Every stored bond: the first page, then the rest at once. */
export async function readBonds(call: Call): Promise<Bond[]> {
  const read = async (offset: number) => (await call<BondPage>(`/api/v1/bonds?limit=${PAGE}&offset=${offset}`)).data
  const first = await read(0)
  const offsets: number[] = []
  for (let offset = PAGE; offset < first.total; offset += PAGE) offsets.push(offset)
  const rest = await Promise.all(offsets.map(read))
  return [first, ...rest].flatMap((page) => page.bonds as unknown as Bond[])
}

/** The desk's marks, or none when this server takes no desk levels (503). */
export async function readDeskLevels(call: Call): Promise<DeskLevels | null> {
  try {
    return (await call<DeskLevels>('/api/v1/market/levels')).data
  } catch (error) {
    if (failureStatus(error) === 503) return null
    throw error
  }
}

/** A book as `GET /api/v1/books` lists it. */
interface BookListing {
  book_id: string
  effective_until: string | null
}

/** The books a page of the list holds, at most the server's 500. */
const BOOK_PAGE = 500

/**
 * The first `most` books by id that are not retired; none when the caller may not list
 * books (403), which needs every book.
 */
export async function readLiveBookIds(call: Call, most: number): Promise<string[] | null> {
  const bookIds: string[] = []
  let after: string | null = null
  try {
    for (;;) {
      const cursor: string = after === null ? '' : `&after=${encodeURIComponent(after)}`
      const page = (await call<{ books: BookListing[]; next_after: string | null }>(`/api/v1/books?limit=${BOOK_PAGE}${cursor}`)).data
      for (const book of page.books) {
        if (book.effective_until !== null) continue
        bookIds.push(book.book_id)
        if (bookIds.length === most) return bookIds
      }
      if (page.next_after === null) return bookIds
      after = page.next_after
    }
  } catch (error) {
    if (failureStatus(error) === 403) return null
    throw error
  }
}

/** The bond stored as `instrumentId`. */
export async function readBond(call: Call, instrumentId: string): Promise<Bond> {
  return (await call<Bond>(`/api/v1/bonds/${encodeURIComponent(instrumentId)}`)).data
}

/** Every entry of the request `rfqKey`, in order. */
export async function readRfqRecords(call: Call, rfqKey: string): Promise<RfqRecord[]> {
  return (await call<{ records: RfqRecord[] }>(`/api/v1/rfqs/${rfqKey}`)).data.records
}

/** Each limit of a book's current set, evaluated now. */
export async function readLimitStatus(call: Call, bookId: string): Promise<LimitEvaluation[]> {
  return (await call<{ limits: LimitEvaluation[] }>(`/api/v1/books/${encodeURIComponent(bookId)}/limits/status`)).data.limits
}

/** A book's P&L since its latest close, by position. */
export async function readBookPnl(call: Call, bookId: string): Promise<BookPnlDetail> {
  return (await call<BookPnlDetail>(`/api/v1/books/${encodeURIComponent(bookId)}/pnl`)).data
}
