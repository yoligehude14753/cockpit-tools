import { create } from 'zustand';

export type SideNavLayoutMode = 'original' | 'classic';

const SIDE_NAV_LAYOUT_STORAGE_KEY = 'agtools.side_nav.layout.v1';
const SIDE_NAV_CLASSIC_COLLAPSED_STORAGE_KEY = 'agtools.side_nav.classic_collapsed.v1';
const SIDE_NAV_HIDE_CLASSIC_SWITCH_PROMPT_KEY = 'agtools.side_nav.hide_classic_switch_prompt.v1';
const SIDE_NAV_CLASSIC_FIRST_SYNC_DONE_KEY = 'agtools.side_nav.classic_first_sync_done.v1';

interface SideNavLayoutState {
  mode: SideNavLayoutMode;
  classicCollapsed: boolean;
  hideClassicSwitchPrompt: boolean;
  classicFirstSyncDone: boolean;
  setMode: (mode: SideNavLayoutMode) => void;
  setClassicCollapsed: (collapsed: boolean) => void;
  toggleClassicCollapsed: () => void;
  setHideClassicSwitchPrompt: (hidden: boolean) => void;
  markClassicFirstSyncDone: () => void;
}

function loadInitialMode(): SideNavLayoutMode {
  if (typeof window === 'undefined') {
    return 'original';
  }

  try {
    const raw = localStorage.getItem(SIDE_NAV_LAYOUT_STORAGE_KEY);
    return raw === 'classic' ? 'classic' : 'original';
  } catch {
    return 'original';
  }
}

function persistMode(mode: SideNavLayoutMode) {
  if (typeof window === 'undefined') {
    return;
  }

  try {
    localStorage.setItem(SIDE_NAV_LAYOUT_STORAGE_KEY, mode);
  } catch {
    // ignore persistence errors
  }
}

function loadInitialClassicCollapsed(): boolean {
  if (typeof window === 'undefined') {
    return false;
  }

  try {
    return localStorage.getItem(SIDE_NAV_CLASSIC_COLLAPSED_STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}

function persistClassicCollapsed(collapsed: boolean) {
  if (typeof window === 'undefined') {
    return;
  }

  try {
    localStorage.setItem(
      SIDE_NAV_CLASSIC_COLLAPSED_STORAGE_KEY,
      collapsed ? '1' : '0',
    );
  } catch {
    // ignore persistence errors
  }
}

function loadInitialHideClassicSwitchPrompt(): boolean {
  if (typeof window === 'undefined') {
    return false;
  }

  try {
    return localStorage.getItem(SIDE_NAV_HIDE_CLASSIC_SWITCH_PROMPT_KEY) === '1';
  } catch {
    return false;
  }
}

function persistHideClassicSwitchPrompt(hidden: boolean) {
  if (typeof window === 'undefined') {
    return;
  }

  try {
    localStorage.setItem(
      SIDE_NAV_HIDE_CLASSIC_SWITCH_PROMPT_KEY,
      hidden ? '1' : '0',
    );
  } catch {
    // ignore persistence errors
  }
}

function loadInitialClassicFirstSyncDone(): boolean {
  if (typeof window === 'undefined') {
    return false;
  }

  try {
    return localStorage.getItem(SIDE_NAV_CLASSIC_FIRST_SYNC_DONE_KEY) === '1';
  } catch {
    return false;
  }
}

function persistClassicFirstSyncDone(done: boolean) {
  if (typeof window === 'undefined') {
    return;
  }

  try {
    localStorage.setItem(SIDE_NAV_CLASSIC_FIRST_SYNC_DONE_KEY, done ? '1' : '0');
  } catch {
    // ignore persistence errors
  }
}

export const useSideNavLayoutStore = create<SideNavLayoutState>((set) => ({
  mode: loadInitialMode(),
  classicCollapsed: loadInitialClassicCollapsed(),
  hideClassicSwitchPrompt: loadInitialHideClassicSwitchPrompt(),
  classicFirstSyncDone: loadInitialClassicFirstSyncDone(),
  setMode: (mode) => {
    persistMode(mode);
    set({ mode });
  },
  setClassicCollapsed: (classicCollapsed) => {
    persistClassicCollapsed(classicCollapsed);
    set({ classicCollapsed });
  },
  toggleClassicCollapsed: () =>
    set((state) => {
      const next = !state.classicCollapsed;
      persistClassicCollapsed(next);
      return { classicCollapsed: next };
    }),
  setHideClassicSwitchPrompt: (hideClassicSwitchPrompt) => {
    persistHideClassicSwitchPrompt(hideClassicSwitchPrompt);
    set({ hideClassicSwitchPrompt });
  },
  markClassicFirstSyncDone: () => {
    persistClassicFirstSyncDone(true);
    set({ classicFirstSyncDone: true });
  },
}));
