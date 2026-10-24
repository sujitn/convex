import { restartState } from '../lib/admin'
import { useRecentActivations, useRuntimeHealth } from '../lib/queries'
import { Badge, ErrorNotice } from './ui'

/** Whether the running work accepted the configuration's active revision (D39). */
export function PendingRestart({ configurationId }: { configurationId: string | null }) {
  const health = useRuntimeHealth()
  const activations = useRecentActivations(configurationId)
  if (!health.data || !configurationId) return null
  if (activations.error) return <ErrorNotice error={activations.error} />
  if (configurationId !== health.data.configuration_id) return null
  const active = activations.data?.items[0]?.revision ?? null
  const state = restartState(health.data, active)
  const text = {
    unknown: 'No activation is recorded for this configuration.',
    current: `The runtime runs the active revision ${active}.`,
    applying: `The runtime is applying revision ${active}.`,
    pending_restart: `Revision ${active} is active and applies at the next restart; the runtime still runs revision ${
      health.data.activation_revision ?? 'none'
    }. The server never restarts itself.`,
  }[state]
  const tone = state === 'pending_restart' ? 'warn' : state === 'current' ? 'good' : 'neutral'
  return (
    <p className="flex items-center gap-2 text-sm">
      <Badge tone={tone}>{state.replace('_', ' ')}</Badge>
      {text}
    </p>
  )
}
