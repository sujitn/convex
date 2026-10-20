// Display formatting. Numbers arrive either as binary64 values or as exact
// decimal strings (cash amounts); both are rendered without inventing digits.

const nf = new Map();
function numberFormat(dp) {
  if (!nf.has(dp)) {
    nf.set(dp, new Intl.NumberFormat('en-US', { minimumFractionDigits: dp, maximumFractionDigits: dp }));
  }
  return nf.get(dp);
}

export function toNumber(v) {
  if (v === null || v === undefined || v === '') return NaN;
  return typeof v === 'number' ? v : Number(v);
}

export function fmt(v, dp = 2) {
  const n = toNumber(v);
  return Number.isFinite(n) ? numberFormat(dp).format(n) : '—';
}

export function signed(v, dp = 2) {
  const n = toNumber(v);
  if (!Number.isFinite(n)) return '—';
  const s = numberFormat(dp).format(Math.abs(n));
  return n > 0 ? `+${s}` : n < 0 ? `−${s}` : s;
}

/** Decimal rate (0.0512) → "5.120" percent text. */
export const pct = (rate, dp = 3) => fmt(toNumber(rate) * 100, dp);
/** Decimal rate → basis points. */
export const bp = (rate, dp = 1) => fmt(toNumber(rate) * 10000, dp);
export const signedBp = (rate, dp = 1) => signed(toNumber(rate) * 10000, dp);

export function money(v, dp = 2) {
  return fmt(v, dp);
}

/** Compact notional: 10000000 → "10mm", 250000 → "250k". */
export function notional(v) {
  const n = toNumber(v);
  if (!Number.isFinite(n)) return '—';
  if (Math.abs(n) >= 1e9) return `${+(n / 1e9).toFixed(2)}bn`;
  if (Math.abs(n) >= 1e6) return `${+(n / 1e6).toFixed(2)}mm`;
  if (Math.abs(n) >= 1e3) return `${+(n / 1e3).toFixed(1)}k`;
  return String(n);
}

export function bytes(n) {
  if (!n) return '0 MB';
  return `${(n / 1048576).toFixed(1)} MB`;
}

export function ms(v) {
  const n = toNumber(v);
  if (!Number.isFinite(n)) return '';
  return n < 1 ? `${n.toFixed(2)} ms` : n < 100 ? `${n.toFixed(1)} ms` : `${Math.round(n)} ms`;
}

export function tenorLabel(years) {
  if (years < 1 / 12 + 1e-9) return `${Math.round(years * 365)}D`;
  if (years < 1 - 1e-9) return `${Math.round(years * 12)}M`;
  return Number.isInteger(+years.toFixed(6)) ? `${Math.round(years)}Y` : `${+years.toFixed(2)}Y`;
}
