// Exact decimal text. Contract terms (coupon, face, call price) cross the
// library boundary as Decimal strings and excess precision is rejected, so
// they are built by moving the decimal point in the user's own digits —
// never by float arithmetic such as (4.875 / 100).toString().

const DECIMAL = /^([+-]?)(\d*)(?:\.(\d*))?$/;

function parts(text) {
  const s = String(text).trim().replace(/,/g, '');
  const m = DECIMAL.exec(s);
  if (!m || (m[2] === '' && (m[3] === undefined || m[3] === ''))) {
    throw new Error(`"${text}" is not a decimal number`);
  }
  return { neg: m[1] === '-', int: m[2] || '0', frac: m[3] || '' };
}

function render(neg, int, frac) {
  const i = int.replace(/^0+(?=\d)/, '') || '0';
  const f = frac.replace(/0+$/, '');
  const body = f ? `${i}.${f}` : i;
  return neg && /[1-9]/.test(body) ? `-${body}` : body;
}

export function isDecimal(text) {
  try {
    parts(text);
    return true;
  } catch {
    return false;
  }
}

/** Canonical decimal text: "+04.5000" → "4.5", "1,000,000" → "1000000". */
export function decimal(text) {
  const { neg, int, frac } = parts(text);
  return render(neg, int, frac);
}

/** Shift the decimal point `places` to the left (positive) or right (negative). */
export function shift(text, places) {
  const { neg, int, frac } = parts(text);
  const digits = int + frac;
  let point = int.length - places;
  let all = digits;
  if (point < 0) {
    all = '0'.repeat(-point) + all;
    point = 0;
  }
  if (point > all.length) all += '0'.repeat(point - all.length);
  return render(neg, all.slice(0, point), all.slice(point));
}

/** "4.875" (percent) → "0.04875" (decimal rate). */
export const percentToRate = (text) => shift(text, 2);
/** "125" (bp) → "0.0125". */
export const bpToRate = (text) => shift(text, 4);
/** "0.04875" → "4.875". */
export const rateToPercent = (text) => shift(text, -2);

/** A finite JS number as the shortest text that round-trips, for computed values. */
export function fromNumber(n, maxFraction = 12) {
  if (!Number.isFinite(n)) throw new Error('Non-finite number');
  const fixed = n.toFixed(maxFraction);
  return decimal(fixed);
}

/**
 * Treasury-style price text to a per-100 number: "99-16" = 99 16/32,
 * "99-16+" = 99 16.5/32, "99-162" = 99 16.25/32, plain decimals pass through.
 * A sign applies to the whole price: "-99-16" = -99.5.
 */
export function parsePrice(text) {
  const s = String(text ?? '').trim();
  const m = /^([+-]?)(\d+)-(\d{2})([+0-7])?$/.exec(s);
  let v;
  if (m) {
    const ticks = Number(m[3]);
    if (ticks >= 32) throw new Error(`"${text}": 32nds must be below 32`);
    const eighths = m[4] === '+' ? 4 : m[4] ? Number(m[4]) : 0;
    v = Number(m[2]) + (ticks + eighths / 8) / 32;
    if (m[1] === '-') v = -v;
  } else {
    // Number() would also take "0x10", "1e2", "Infinity" and blank text.
    if (!/^[+-]?(?:\d+\.?\d*|\.\d+)$/.test(s)) throw new Error(`"${text}" is not a price`);
    v = Number(s);
  }
  if (!Number.isFinite(v)) throw new Error(`"${text}" is not a price`);
  return v;
}

/** Per-100 price → "99-16+" (nearest 1/256). */
export function formatThirtySeconds(price) {
  if (!Number.isFinite(price)) return '';
  const neg = price < 0;
  const p = Math.abs(price);
  let whole = Math.floor(p);
  let eighths = Math.round((p - whole) * 256);
  if (eighths === 256) { whole += 1; eighths = 0; }
  const ticks = Math.floor(eighths / 8);
  const rem = eighths % 8;
  const tail = rem === 0 ? '' : rem === 4 ? '+' : String(rem);
  return `${neg ? '-' : ''}${whole}-${String(ticks).padStart(2, '0')}${tail}`;
}
