import { create } from 'zustand';
import type { CoreStatus, TrafficTick } from '../api/core';

const TRAFFIC_KEEP = 60; // ~60 seconds at 1Hz

interface CoreStoreState {
  status: CoreStatus | null;
  /** Most recent first? No — chronological for recharts (left-to-right). */
  traffic: TrafficTick[];
  setStatus: (s: CoreStatus | null) => void;
  pushTraffic: (t: TrafficTick) => void;
  resetTraffic: () => void;
}

/** Drop pump events stamped with an epoch lower than the latest core_status
 * we know about. Without this guard, an in-flight emit from the previous
 * sing-box session can land on the chart after restart and look like real
 * traffic. */
function isCurrentEpoch(status: CoreStatus | null, epoch: number): boolean {
  if (!status) return true; // no status yet — accept; status fetch will follow
  return epoch === status.epoch;
}

export const useCoreStore = create<CoreStoreState>((set, get) => ({
  status: null,
  traffic: [],
  setStatus: (status) =>
    set((s) => {
      // When the epoch advances, drop the old chart points. The new
      // session's first emit will refill it.
      if (status && s.status && status.epoch !== s.status.epoch) {
        return { status, traffic: [] };
      }
      return { status };
    }),
  pushTraffic: (t) => {
    if (!isCurrentEpoch(get().status, t.epoch)) return;
    set((s) => {
      const next = s.traffic.length >= TRAFFIC_KEEP ? s.traffic.slice(1) : s.traffic.slice();
      next.push(t);
      return { traffic: next };
    });
  },
  resetTraffic: () => set({ traffic: [] }),
}));
