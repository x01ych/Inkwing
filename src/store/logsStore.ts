import { create } from 'zustand';
import type { LogEntry } from '../api/logs';

const KEEP = 2000;

interface LogsStoreState {
  entries: LogEntry[];
  paused: boolean;
  pendingWhilePaused: LogEntry[];
  /** Latest session epoch the store has accepted. Batches stamped with a
   * lower epoch are silently dropped (carry-over from a stopped session). */
  epoch: number;
  hydrate: (entries: LogEntry[]) => void;
  appendBatch: (batch: LogEntry[], epoch: number) => void;
  setPaused: (paused: boolean) => void;
  clear: () => void;
  /** Called when the active core epoch advances — clear stale entries. */
  setEpoch: (epoch: number) => void;
}

export const useLogsStore = create<LogsStoreState>((set) => ({
  entries: [],
  paused: false,
  pendingWhilePaused: [],
  epoch: 0,
  hydrate: (entries) => set({ entries: entries.slice(-KEEP), pendingWhilePaused: [] }),
  appendBatch: (batch, epoch) =>
    set((s) => {
      // Out-of-session straggler: drop. (epoch == 0 means "not yet learned"
      // — accept defensively, the upcoming setEpoch will resync.)
      if (s.epoch !== 0 && epoch !== s.epoch) return {};
      if (s.paused) {
        const buf = s.pendingWhilePaused.concat(batch);
        return { pendingWhilePaused: buf.slice(-KEEP) };
      }
      const next = s.entries.concat(batch);
      return { entries: next.length > KEEP ? next.slice(next.length - KEEP) : next };
    }),
  setPaused: (paused) =>
    set((s) => {
      if (!paused && s.pendingWhilePaused.length) {
        const next = s.entries.concat(s.pendingWhilePaused);
        return {
          paused: false,
          entries: next.length > KEEP ? next.slice(next.length - KEEP) : next,
          pendingWhilePaused: [],
        };
      }
      return { paused };
    }),
  clear: () => set({ entries: [], pendingWhilePaused: [] }),
  setEpoch: (epoch) =>
    set((s) => {
      if (epoch === s.epoch) return {};
      // New session: discard old entries and pending buffer.
      return { epoch, entries: [], pendingWhilePaused: [] };
    }),
}));
