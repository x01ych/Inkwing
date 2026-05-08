import { invoke } from './tauri';

export type ProxyMode = 'rule' | 'global' | 'direct';

export type ThemeColor = 'zinc' | 'slate' | 'blue' | 'green' | 'rose';

export interface Settings {
  minimize_to_tray: boolean;
  autostart: boolean;
  latency_test_url: string;
  language: string;
  theme: string;
  /** shadcn-style color palette name. Persisted; applied via class on
   * <html> by App.tsx so all CSS variables flip in one frame. */
  theme_color: ThemeColor;
  tun_enabled: boolean;
  proxy_mode: ProxyMode;
  mixed_port: number | null;
  socks_port: number | null;
  http_port: number | null;
}

export const settingsApi = {
  get: () => invoke<Settings>('settings_get'),
  set: (patch: Partial<Settings>) =>
    invoke<Settings>('settings_set', { patch }),
};
