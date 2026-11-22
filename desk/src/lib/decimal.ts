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

/** `a × n` for a whole `n`, exactly, at `a`'s scale. */
export function multiplyDecimal(a: string, n: number): string {
  if (!Number.isInteger(n)) throw new Error(`not a whole multiple: ${n}`)
  const scale = scaleOf(a)
  return written(scaled(a, scale) * BigInt(n), scale)
}
