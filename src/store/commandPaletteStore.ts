/**
 * Command Palette Store
 *
 * Minimal store to lift command palette open/close state out of App.tsx
 * so that other components can trigger it.
 */

import { create } from 'zustand';

interface CommandPaletteState {
  open: boolean;
  setOpen: (open: boolean) => void;
}

export const useCommandPaletteStore = create<CommandPaletteState>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}));
