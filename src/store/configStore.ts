import { create } from 'zustand';
import type { ConfigSummary, ValidationReport } from '../api/config';

interface ConfigStoreState {
  summary: ConfigSummary | null;
  raw: string;
  lastValidation: ValidationReport | null;
  setSummary: (s: ConfigSummary | null) => void;
  setRaw: (raw: string) => void;
  setValidation: (r: ValidationReport | null) => void;
  reset: () => void;
}

export const useConfigStore = create<ConfigStoreState>((set) => ({
  summary: null,
  raw: '',
  lastValidation: null,
  setSummary: (summary) => set({ summary }),
  setRaw: (raw) => set({ raw }),
  setValidation: (lastValidation) => set({ lastValidation }),
  reset: () => set({ summary: null, raw: '', lastValidation: null }),
}));
