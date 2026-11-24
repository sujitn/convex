// Exact decimal arithmetic on the server's decimal strings. A level is sent back as an
// exact string, so a nudge never passes through a binary float: 0.0525 plus one basis
// point is 0.0526, not 0.052600000000000001.

const DECIMAL = /^-?(\d+)(?:\.(\d+))?$/

/** Whether `text` is a plain decimal the server accepts: digits, one optional point. */
export function isDecimal(text: string): boolean {
  return DECIMAL.test(text)
}

function scaleOf(text: string): number {
  const point = text.indexOf('.')
  return point === -1 ? 0 : text.length - point - 1
}

function scaled(text: string, scale: number): bigint {
  const match = DECIMAL.exec(text)
  if (!match) throw new Error(`not a decimal: ${text}`)
  const fraction = (match[2] ?? '').padEnd(scale, '0')
  const magnitude = BigInt(`${match[1]}${fraction}`)
  return text.startsWith('-') ? -magnitude : magnitude
}

function written(value: bigint, scale: number): string {
  const negative = value < 0n
  const digits = (negative ? -value : value).toString().padStart(scale + 1, '0')
  const whole = digits.slice(0, digits.length - scale)
  const fraction = scale > 0 ? `.${digits.slice(digits.length - scale)}` : ''
  return `${negative ? '-' : ''}${whole}${fraction}`
}

/** `a + b`, exactly, at the larger of their two scales. */
export function addDecimal(a: string, b: string): string {
  const scale = Math.max(scaleOf(a), scaleOf(b))
  return written(scaled(a, scale) + scaled(b, scale), scale)
}

/** Whether `text` is a decimal above zero. */
export function isPositiveDecimal(text: string): boolean {
  return isDecimal(text) && !text.startsWith('-') && /[1-9]/.test(text)
}

/**
 * `text` rounded to a multiple of `tick`, down or up, exactly, written at the tick's scale
 * but without trailing zeros past three places: 99.2437 to the 0.001 below is 99.243, and
 * to the 1/256 (0.00390625) above is 99.24609375.
 */
export function roundToTick(text: string, tick: string, direction: 'down' | 'up'): string {
  const scale = Math.max(scaleOf(text), scaleOf(tick))
  const value = scaled(text, scale)
  const step = scaled(tick, scale)
  let ticks = value / step
  const remainder = value % step
  if (remainder !== 0n && (direction === 'up') === remainder > 0n) ticks += direction === 'up' ? 1n : -1n
  const rounded = written(ticks * step, scale)
  const trimmed = rounded.replace(/0+$/, '')
  const point = trimmed.indexOf('.')
  return point === -1 || trimmed.length - point - 1 >= 3 ? trimmed : rounded.slice(0, point + 4)
}
