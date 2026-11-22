// Wire shapes the screens read. Only the fields the screens use are named; every
// record is still shown whole as JSON.

export interface RuntimeHealth {
  phase: string
  ready: boolean
  mode: string | null
  quality: string
  failed_instruments: number
  snapshot_id: string | null
  generation: number
  revision: number
  pending_publications: number
  replay_complete: boolean
  failure: string | null
  configuration_id: string | null
  configuration_version: number | null
  activation_revision: number | null
  desired_activation_revision: number | null
}

export interface Dependencies {
  ready: boolean
  runtime_ready: boolean
  strategy_ready: boolean
  dependencies: { name: string; required: boolean; state: string }[]
}

export interface CurvePage {
  curves: { curve_id: string; version: number; recorded_at: string }[]
  next: string | null
}

export interface BookPage {
  books: {
    book_id: string
    name: string
    owning_system: string
    position_count: number
    version: number
    recorded_at: string
  }[]
  next_after: string | null
}

export interface ActivationRecord {
  revision: number
  previous_version: number | null
  configuration_digest: string
  command: {
    action: { kind: string; to_revision?: number }
    configuration_version: number
    mode: string
    actor: string
    reason: string
    activated_at: string
    fixture_evidence?: string
  }
}

export interface HistoryPage {
  items: ActivationRecord[]
  total: number
  offset: number
  limit: number
}

export interface StagedVersion {
  configuration: unknown
  digest: string
  actor: string
  reason: string
  staged_at: string
  idempotency_key: string
  fixture_evidence: string
}

export interface VersionDiff {
  from: number
  to: number
  changes: { field: string; reload: string }[]
}

export interface ActivationOutcome {
  activation: ActivationRecord
  accepted_generation: number | null
  accepted_activation_revision: number | null
  restart_required: boolean
}
