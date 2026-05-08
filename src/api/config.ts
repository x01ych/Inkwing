import { invoke } from './tauri';

export interface InboundSummary {
  type: string;
  tag: string | null;
}

export interface ConfigSummary {
  path: string;
  size_bytes: number;
  inbounds: InboundSummary[];
  outbound_tags: string[];
  rule_count: number;
  final_outbound: string | null;
  has_clash_api: boolean;
  has_tun: boolean;
  log_level: string | null;
}

export interface ValidationError {
  level: string;
  message: string;
}

export interface ValidationReport {
  ok: boolean;
  exit_code: number | null;
  errors: ValidationError[];
}

// ---- library --------------------------------------------------------------

export type ConfigSource =
  | { kind: 'local'; original_path: string | null }
  | { kind: 'subscription'; sub_id: string; fetched_at_ms: number };

export interface ConfigEntry {
  id: string;
  name: string;
  source: ConfigSource;
  storage_path: string;
  created_at_ms: number;
  updated_at_ms: number;
}

export interface ConfigEntrySummary extends ConfigEntry {
  is_active: boolean;
  outbound_count: number | null;
  rule_count: number | null;
  has_tun_inbound: boolean | null;
}

export const configApi = {
  openDialog: () => invoke<string | null>('config_open_dialog'),
  load: (path: string) => invoke<ConfigSummary>('config_load', { path }),
  validate: (path?: string) =>
    invoke<ValidationReport>('config_validate', { path: path ?? null }),
  getRaw: () => invoke<string>('config_get_raw'),
  save: (content: string) => invoke<void>('config_save', { content }),
  reveal: () => invoke<void>('config_reveal'),
  currentPath: () => invoke<string | null>('config_current_path'),

  // library
  libraryList: () => invoke<ConfigEntrySummary[]>('config_library_list'),
  libraryAddLocal: (path: string) =>
    invoke<ConfigEntry>('config_library_add_local', { path }),
  libraryAddFromText: (name: string, text: string) =>
    invoke<ConfigEntry>('config_library_add_from_text', { name, text }),
  libraryRemove: (id: string) =>
    invoke<void>('config_library_remove', { id }),
  libraryRename: (id: string, newName: string) =>
    invoke<void>('config_library_rename', { id, newName }),
  /** Select an entry as active. Sing-box always (re)starts after this.
   * Returns the new active ConfigSummary so the caller can update its
   * local store without an extra round-trip. */
  librarySelect: (id: string) =>
    invoke<ConfigSummary | null>('config_library_select', { id }),
  /** Build a ConfigSummary from whatever's currently active in the
   * backend cache. None when nothing is active. Used by App.tsx at boot
   * and after `library:changed` events. */
  activeSummary: () => invoke<ConfigSummary | null>('config_active_summary'),
  libraryView: (id: string) => invoke<string>('config_library_view', { id }),
  libraryReveal: (id: string) =>
    invoke<void>('config_library_reveal', { id }),
  /** In-place re-fetch from the entry's source subscription. Overwrites
   * the entry's storage_path with the new content; if this entry was the
   * active one, sing-box restarts automatically. Local-source entries
   * cannot be refreshed (no subscription to fetch from). */
  libraryRefreshFromSubscription: (id: string) =>
    invoke<void>('config_library_refresh_from_subscription', { id }),
};
