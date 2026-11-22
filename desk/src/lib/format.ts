// How the desk reads numbers: prices per 100 in decimals or, for Treasuries, in 32nds;
// yields in percent; spreads and margins in basis points. Display only: what is sent
// back is the exact level string, never a formatted one.

/** A published value: an exact decimal string, or a float for a model output. */
export type WireNumber = number | string

function numeric(value: WireNumber): number {
  return typeof value === 'number' ? value : Number(value)
}

/** A price per 100 at three decimals, as a corporate desk quotes it. */
export function formatPrice(value: WireNumber): string {
  return numeric(value).toFixed(3)
}

/**
 * A price per 100 in 32nds, as Treasuries trade: `99-16` is 99 and 16/32, `99-16+`
 * adds a half, and a third digit counts eighths of a 32nd (`99-162` is 16 2/8). The
 * price is rounded to the nearest 256th.
 */
export function formatThirtySeconds(value: WireNumber): string {
  const price = numeric(value)
  const sign = price < 0 ? '-' : ''
  const totalEighths = Math.round(Math.abs(price) * 256)
  const handle = Math.floor(totalEighths / 256)
  const eighths = totalEighths % 256
  const ticks = Math.floor(eighths / 8)
  const remainder = eighths % 8
  const fraction = remainder === 0 ? '' : remainder === 4 ? '+' : String(remainder)
  return `${sign}${handle}-${String(ticks).padStart(2, '0')}${fraction}`
}

/** A decimal yield (0.0525) as percent at three decimals (5.250). */
export function formatYieldPercent(value: WireNumber): string {
  return (numeric(value) * 100).toFixed(3)
}

/**
 * Basis points, signed when negative: a model output at one decimal, and an exact
 * decimal, such as a desk mark of 140.25, as written to at most two.
 */
export function formatBasisPoints(value: WireNumber): string {
  if (typeof value === 'number') return value.toFixed(1)
  const decimals = value.includes('.') ? value.length - value.indexOf('.') - 1 : 0
  return Number(value).toFixed(Math.min(2, Math.max(1, decimals)))
}

/** A coupon rate held as a decimal (0.04125) as percent, trailing zeros trimmed (4.125). */
export function formatCoupon(value: WireNumber | null | undefined): string {
  if (value === null || value === undefined) return '—'
  return String(Number((numeric(value) * 100).toFixed(4)))
}

/** A measure as the desk reads it, by its unit. */
export function formatMeasure(value: WireNumber, unit: string, thirtySeconds = false): string {
  switch (unit) {
    case 'price_per_100':
      return thirtySeconds ? formatThirtySeconds(value) : formatPrice(value)
    case 'decimal_rate':
      return formatYieldPercent(value)
    case 'bp':
      return formatBasisPoints(value)
    default:
      return String(typeof value === 'number' ? Number(value.toPrecision(8)) : value)
  }
}

/**
 * The unit a reply's flat measure is in, by the server's names: prices and accrued per
 * 100, yields as decimal rates, spreads and margins in basis points; null for a risk
 * measure such as a duration or DV01, shown as it is.
 */
export function measureUnit(name: string): 'price_per_100' | 'decimal_rate' | 'bp' | null {
  if (/duration|convexity|dv01|pv01|cs01/.test(name)) return null
  if (/price|accrued/.test(name)) return 'price_per_100'
  if (/^(ytm|ytw|ytc|yield|current_yield)/.test(name)) return 'decimal_rate'
  if (/spread|margin|^oas|asw/.test(name)) return 'bp'
  return null
}

/** How long ago `then` was, coarsely, for a mark's age. */
export function formatAge(then: string, now: number): string {
  const seconds = Math.max(0, Math.round((now - Date.parse(then)) / 1000))
  if (seconds < 60) return `${seconds}s`
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`
  return `${Math.floor(seconds / 3600)}h`
}

/** The calendar date of `at` where the page runs, as `YYYY-MM-DD`. */
export function localDate(at: Date): string {
  const month = String(at.getMonth() + 1).padStart(2, '0')
  const day = String(at.getDate()).padStart(2, '0')
  return `${at.getFullYear()}-${month}-${day}`
}
