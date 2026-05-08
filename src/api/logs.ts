import { invoke } from './tauri';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface LogEntry {
  ts_ms: number;
  level: string;
  payload: string;
}

export interface LogBatch {
  epoch: number;
  entries: LogEntry[];
}

export const logsApi = {
  recent: () => invoke<LogEntry[]>('logs_recent'),
};

export function onLogsAppend(cb: (batch: LogBatch) => void): Promise<UnlistenFn> {
  return listen<LogBatch>('logs:append', (e) => cb(e.payload));
}
