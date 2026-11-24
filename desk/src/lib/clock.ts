import { useSyncExternalStore } from 'react'

// One clock for every cell that reads the time, so a second's tick re-renders those cells
// only; and the server's clock, as the stream stamps it, for the deadlines it enforces.

const clock = (() => {
  let now = Date.now()
  const listeners = new Set<() => void>()
  let timer: ReturnType<typeof setInterval> | undefined
  return {
    subscribe(listener: () => void) {
      listeners.add(listener)
      // The time may be old if nothing read it since the last tick.
      if (timer === undefined) now = Date.now()
      timer ??= setInterval(() => {
        now = Date.now()
        listeners.forEach((notify) => notify())
      }, 1000)
      return () => {
        listeners.delete(listener)
        if (listeners.size === 0) {
          clearInterval(timer)
          timer = undefined
        }
      }
    },
    now: () => now,
  }
})()

/** The page's time, in milliseconds since the epoch, moving once a second. */
export function useNow(): number {
  return useSyncExternalStore(clock.subscribe, clock.now)
}

/** The server's clock less the page's, in milliseconds. */
let serverOffset = 0

/** A time the server stamped on a message, in milliseconds since the epoch. */
export function observeServerTime(serverTimeMs: number): void {
  serverOffset = serverTimeMs - Date.now()
}

/** The server's time now, in milliseconds since the epoch. */
export function serverNow(): number {
  return Date.now() + serverOffset
}

/** The server's time, moving once a second. */
export function useServerNow(): number {
  return useNow() + serverOffset
}
