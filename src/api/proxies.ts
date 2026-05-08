import { invoke } from './tauri';

/** sing-box exposes the Mihomo /proxies shape:
 *   { proxies: { [name: string]: ProxyEntry } }
 * Selectors/url-test groups have `now` (active) and `all` (members).
 * Plain outbounds (direct/block/concrete proxies) just have type+history. */
export interface ProxyEntry {
  type: string;
  name: string;
  now?: string;
  all?: string[];
  udp?: boolean;
  history?: { time: string; delay: number }[];
}

export interface ProxiesResponse {
  proxies: Record<string, ProxyEntry>;
}

export interface DelayResult {
  delay_ms: number | null;
}

export interface GroupTestResult {
  name: string;
  delay_ms: number | null;
}

export interface SpeedTestResult {
  bytes: number;
  duration_ms: number;
  bytes_per_sec: number;
}

export const proxiesApi = {
  list: () => invoke<ProxiesResponse>('proxies_list'),
  select: (group: string, name: string) =>
    invoke<void>('proxies_select', { group, name }),
  test: (name: string, url: string, timeoutMs: number) =>
    invoke<DelayResult>('proxies_test', { name, url, timeoutMs }),
  testMany: (names: string[], url: string, timeoutMs: number) =>
    invoke<GroupTestResult[]>('proxies_test_many', { names, url, timeoutMs }),
  speedtest: (group: string, name: string, url: string, maxBytes: number) =>
    invoke<SpeedTestResult>('proxies_speedtest', {
      group,
      name,
      url,
      maxBytes,
    }),
};
