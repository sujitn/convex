import type { BondPage, Reply } from '@convex/api-client'
import type { Bond, DeskLevels, PricingRule } from './sheet'

// The sheet's reads: every pricing rule and bond, a page at a time, and the desk's marks.

/** The most records the server returns a reference page. */
const PAGE = 1000

type Call = <T>(path: string) => Promise<Reply<T>>

interface MasterPage {
  items: { definition: { kind: string; definition: PricingRule } }[]
  next: string | null
}

/** Every stored pricing rule, in instrument id order. */
export async function readPricingRules(call: Call): Promise<PricingRule[]> {
  const rules: PricingRule[] = []
  let after: string | null = null
  for (;;) {
    const cursor: string = after === null ? '' : `&after=${encodeURIComponent(after)}`
    const page: MasterPage = (await call<MasterPage>(`/api/v1/reference/masters/pricing_rule?limit=${PAGE}${cursor}`)).data
    rules.push(...page.items.map((item) => item.definition.definition))
    if (page.next === null) return rules
    after = page.next
  }
}

/** Every stored bond. */
export async function readBonds(call: Call): Promise<Bond[]> {
  const bonds: Bond[] = []
  for (let offset = 0; ; offset += PAGE) {
    const page = (await call<BondPage>(`/api/v1/bonds?limit=${PAGE}&offset=${offset}`)).data
    bonds.push(...(page.bonds as unknown as Bond[]))
    if (page.bonds.length < PAGE || offset + PAGE >= page.total) return bonds
  }
}

/** The desk's marks, or none when this server takes no desk levels (503). */
export async function readDeskLevels(call: Call): Promise<DeskLevels | null> {
  try {
    return (await call<DeskLevels>('/api/v1/market/levels')).data
  } catch (error) {
    if ((error as { status?: number }).status === 503) return null
    throw error
  }
}
