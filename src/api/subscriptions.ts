import { invoke } from './tauri';

export interface Subscription {
  id: string;
  name: string;
  url: string;
  /** 0 = no every-N-hours mode. Ignored when `daily_update_at` is set. */
  interval_hours: number;
  /** "HH:MM" 24-hour local time. When set, this wins over `interval_hours`. */
  daily_update_at: string | null;
  last_fetched_at_ms: number | null;
  last_error: string | null;
  outbound_count: number | null;
  /** After a scheduled apply, keep the most-recent N entries from this
   * subscription and delete the rest (active is never deleted). */
  keep_last_n: number | null;
  /** When true, a scheduled apply that lands while the currently-active
   * config came from this subscription will switch active → new entry
   * and restart sing-box. */
  auto_switch_to_new: boolean;
  /** Count of consecutive scheduled-apply failures. */
  consecutive_failures: number;
}

export interface SubInput {
  name: string;
  url: string;
  interval_hours?: number;
  daily_update_at?: string | null;
  keep_last_n?: number | null;
  auto_switch_to_new?: boolean | null;
}

export const subsApi = {
  list: () => invoke<Subscription[]>('subs_list'),
  add: (input: SubInput) => invoke<Subscription>('subs_add', { input }),
  update: (id: string, input: SubInput) =>
    invoke<Subscription>('subs_update', { id, input }),
  remove: (id: string) => invoke<void>('subs_remove', { id }),
  refresh: (id: string) => invoke<Subscription>('subs_refresh', { id }),
  /** Fetch the subscription URL and add the result as a NEW config in
   * the library. Returns the new ConfigEntry id. Active config is NOT
   * changed. */
  apply: (id: string) => invoke<string>('subs_apply', { id }),
};
