import { failureNotice } from './failure'
import type { DeskMark, LevelField } from './sheet'

// This page's writes of the desk's marks, and the order of each read of the marks
// against them. Apart from the screen, so the queue and the rollback are tested.

/** Send one level; resolves with when the server took it, in milliseconds since the epoch. */
export type PostMark = (instrumentId: string, field: LevelField, level: string) => Promise<number>

/** A bond the desk quotes: its level's field, and the server's last mark as the page knows it. */
export interface DeskBond {
  instrumentId: string
  field: LevelField
  acknowledged: DeskMark | null
}

export interface MarkWriterHandlers {
  /** Show `mark` as the bond's level: the level set, then the server's last once none follows. */
  onShown: (instrumentId: string, mark: DeskMark | null) => void
  /** Why the last mark was refused, or none once one is taken. */
  onNotice: (notice: string | null) => void
}

/**
 * One mark in flight a bond. A level set meanwhile waits and only the latest is sent,
 * since a level is absolute. Once none follows, the bond shows the server's last mark,
 * so a refused mark reverts to it, never to a level only queued.
 */
export class MarkWriter {
  /** Bonds with a mark in flight, each with the level queued behind it. */
  private readonly inFlight = new Map<string, string | null>()
  /** When the server last took each bond's mark, on this page's clock. */
  private readonly acknowledgedAt = new Map<string, number>()

  constructor(
    private readonly handlers: MarkWriterHandlers,
    private readonly now: () => number = () => performance.now(),
  ) {}

  /** Set the bond's level, sent with `post`. */
  mark(bond: DeskBond, level: string, post: PostMark): void {
    this.handlers.onShown(bond.instrumentId, { level, marked_at: new Date().toISOString() })
    if (this.inFlight.has(bond.instrumentId)) {
      this.inFlight.set(bond.instrumentId, level)
      return
    }
    void this.send(bond, level, post)
  }

  /** The time a read of the marks is issued, to order it against the marks acknowledged. */
  readIssued(): number {
    return this.now()
  }

  /**
   * The bonds a read of the marks issued at `issuedAt` may not reflect: a mark in flight,
   * or one the server took after the read was issued.
   */
  unread(issuedAt: number): Set<string> {
    const unread = new Set(this.inFlight.keys())
    for (const [instrumentId, at] of this.acknowledgedAt) if (at >= issuedAt) unread.add(instrumentId)
    return unread
  }

  private async send({ instrumentId, field, acknowledged }: DeskBond, level: string, post: PostMark) {
    let last = acknowledged
    let next: string | null | undefined = level
    while (next) {
      this.inFlight.set(instrumentId, null)
      try {
        const receivedAtMs = await post(instrumentId, field, next)
        this.acknowledgedAt.set(instrumentId, this.now())
        // Stamped with the server's receipt time, as the marks read back are.
        last = { level: next, marked_at: new Date(receivedAtMs).toISOString() }
        this.handlers.onNotice(null)
      } catch (error) {
        this.handlers.onNotice(failureNotice(error))
      }
      next = this.inFlight.get(instrumentId)
    }
    this.inFlight.delete(instrumentId)
    this.handlers.onShown(instrumentId, last)
  }
}
