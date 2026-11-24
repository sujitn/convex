import type { StreamStatus } from '../lib/stream'

/** The stream's state: open, connecting or reconnecting, or idle before it is opened. */
export function StreamBadge({ status }: { status: StreamStatus | 'idle' }) {
  const tone = status === 'open' ? 'text-emerald-400' : status === 'idle' ? 'text-slate-500' : 'text-amber-300'
  return <span className={tone}>{status}</span>
}
