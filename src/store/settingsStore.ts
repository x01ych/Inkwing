import { create } from 'zustand';
import type { Settings } from '../api/settings';

interface SettingsStoreState {
  settings: Settings | null;
  setSettings: (s: Settings) => void;
}

/** Loaded once at app boot in App.tsx; pages read from here without
 * re-fetching, so navigating to Settings (or any page that needs the
 * latency-test URL etc.) is instant — no Spin flash. */
export const useSettingsStore = create<SettingsStoreState>((set) => ({
  settings: null,
  setSettings: (settings) => set({ settings }),
}));
