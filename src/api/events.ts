import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { TrafficTick } from './core';

export function onTraffic(cb: (t: TrafficTick) => void): Promise<UnlistenFn> {
  return listen<TrafficTick>('traffic:tick', (e) => cb(e.payload));
}

/** Backend always emits a tagged-object payload now. A "started" event
 * carries the new session epoch; pump events stamp the same epoch and
 * the frontend stores drop anything that doesn't match. */
export type CoreStateEvent =
  | { kind: 'started'; epoch: number }
  | { kind: 'stopped' }
  | { kind: 'crashed'; code: number | null };

export function onCoreState(cb: (state: CoreStateEvent) => void): Promise<UnlistenFn> {
  return listen<CoreStateEvent>('core:state', (e) => cb(e.payload));
}

/** Emitted when one of the three pumps gives up after MAX_CONSEC_FAILS
 * consecutive errors talking to clash_api. Distinct from `core:state
 * crashed`: the sing-box process may still be alive, but our HTTP path
 * to it is broken (auth mismatch, secret rotated, port collision). */
export interface PumpStaleEvent {
  kind: 'logs' | 'traffic' | 'connections';
  epoch: number;
}

export function onPumpStale(cb: (e: PumpStaleEvent) => void): Promise<UnlistenFn> {
  return listen<PumpStaleEvent>('pumps:stale', (e) => cb(e.payload));
}
