import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from './tauri';

export interface ConnMeta {
  network: string;
  type: string;
  sourceIP: string;
  sourcePort: string;
  destinationIP: string;
  destinationPort: string;
  host: string;
  processPath: string;
  process: string;
  /** Sniffed SNI / HTTP Host. Often set even when `host` isn't, for
   * connections sing-box parsed mid-stream. */
  sniffHost?: string;
  /** sing-box-specific: the actual remote target (after rewriting /
   * resolution). When set, more accurate than destinationIP for the
   * "what is this connection actually going to" question. */
  remoteDestination?: string;
  /** Where the user connected to OUR proxy. Useful for distinguishing
   * inbounds, especially when filtering self-traffic. */
  inboundIP?: string;
  inboundPort?: string;
}

export interface ConnRow {
  id: string;
  upload: number;
  download: number;
  start: string;
  chains: string[];
  rule: string;
  rulePayload: string;
  metadata: ConnMeta;
}

export interface ConnSnapshot {
  connections: ConnRow[];
  downloadTotal: number;
  uploadTotal: number;
  memory: number;
  epoch: number;
}

export function onConnectionsSnapshot(
  cb: (s: ConnSnapshot) => void
): Promise<UnlistenFn> {
  return listen<ConnSnapshot>('connections:snapshot', (e) => cb(e.payload));
}

export const connectionsApi = {
  close: (id: string) => invoke<void>('connections_close', { id }),
  closeAll: () => invoke<void>('connections_close_all'),
};
