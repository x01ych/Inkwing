import { create } from 'zustand';

export type TunDialogPlatform = 'windows' | 'macos' | 'linux' | 'other';

interface TunDialogState {
  open: boolean;
  platform: TunDialogPlatform;
  hint: string;
  show: (input: { platform: TunDialogPlatform; hint: string }) => void;
  hide: () => void;
}

export const useTunDialogStore = create<TunDialogState>((set) => ({
  open: false,
  platform: 'other',
  hint: '',
  show: ({ platform, hint }) => set({ open: true, platform, hint }),
  hide: () => set({ open: false }),
}));
