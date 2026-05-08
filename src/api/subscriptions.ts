import { invoke } from './tauri';

export interface Subscription {
  id: string;
  name: string;
  url: string;
  interval_hours: number;
  last_fetched_at_ms: number | null;
  last_error: string | null;
  outbound_count: number | null;
}

export interface SubInput {
  name: string;
  url: string;
  interval_hours?: number;
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
