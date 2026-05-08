import { create } from 'zustand';
import type { ConnRow, ConnSnapshot } from '../api/connections';
import { useCoreStore } from './coreStore';

const CLOSED_KEEP = 200;

/** Drop the noise where sing-box's clash_api includes the *very*
 * connections this app is using to read clash_api itself — the three
 * WS pumps (logs / connections / traffic) plus any HTTP polls. They
 * always show as `dst = 127.0.0.1:<clash_api_port>` because the API
 * server is the destination. Filtering keeps the user's view of "what
 * traffic is sing-box proxying for me" actually about user traffic. */
function isOwnApiTraffic(c: ConnRow, clashApiAddr: string | null | undefined): boolean {
  if (!clashApiAddr) return false;
  const idx = clashApiAddr.lastIndexOf(':');
  if (idx < 0) return false;
  const host = clashApiAddr.slice(0, idx);
  const port = clashApiAddr.slice(idx + 1);
  return c.metadata.destinationIP === host && c.metadata.destinationPort === port;
}

export interface ClosedConn extends ConnRow {
  closed_at_ms: number;
}

interface ConnectionsStoreState {
  snapshot: ConnSnapshot | null;
  closed: ClosedConn[];
  /** Latest session epoch accepted; older snapshots are dropped. */
  epoch: number;
  ingest: (s: ConnSnapshot) => void;
  reset: () => void;
  setEpoch: (epoch: number) => void;
}

/** Diff each incoming snapshot against the previous one; ids that vanished
 * are moved into a bounded `closed` ring (most recent first). The Closed
 * tab reads from there. We keep ~200 rows; high-churn workloads will
 * cycle through quickly — that's deliberate. */
export const useConnectionsStore = create<ConnectionsStoreState>((set, get) => ({
  snapshot: null,
  closed: [],
  epoch: 0,
  ingest: (raw) => {
    const cur = get().epoch;
    if (cur !== 0 && raw.epoch !== cur) return; // stale session

    // Strip our own clash_api WS / HTTP traffic before anything else
    // sees it. Doing it here (in the store) means every consumer —
    // Connections page, Dashboard "active connections" stat, etc. —
    // gets a clean number.
    const clashAddr = useCoreStore.getState().status?.clash_api_addr ?? null;
    const snapshot: ConnSnapshot = {
      ...raw,
      connections: raw.connections.filter((c) => !isOwnApiTraffic(c, clashAddr)),
    };

    const prev = get().snapshot;
    let nextClosed = get().closed;

    // sing-box reuses connection ids in some scenarios (UDP NAT slot
    // recycle, or a fresh connection on a quickly-closed-and-reopened
    // socket). If an id that's currently in `closed` reappears in the
    // live snapshot, drop the closed entry — otherwise the same id
    // shows up in BOTH tabs and the user sees a phantom row.
    const aliveIds = new Set(snapshot.connections.map((c) => c.id));
    if (nextClosed.some((c) => aliveIds.has(c.id))) {
      nextClosed = nextClosed.filter((c) => !aliveIds.has(c.id));
    }

    if (prev) {
      const justClosed: ClosedConn[] = prev.connections
        .filter((c) => !aliveIds.has(c.id))
        .map((c) => ({ ...c, closed_at_ms: Date.now() }));
      if (justClosed.length) {
        nextClosed = justClosed.concat(nextClosed).slice(0, CLOSED_KEEP);
      }
    }

    set({ snapshot, closed: nextClosed });
  },
  reset: () => set({ snapshot: null, closed: [] }),
  setEpoch: (epoch) =>
    set((s) => {
      if (epoch === s.epoch) return {};
      // New session: stale snapshot/closed must go.
      return { epoch, snapshot: null, closed: [] };
    }),
}));
