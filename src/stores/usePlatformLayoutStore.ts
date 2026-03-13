import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { ALL_PLATFORM_IDS, PlatformId } from '../types/platform';

const PLATFORM_LAYOUT_STORAGE_KEY = 'agtools.platform_layout.v1';
const LEGACY_TRAY_CORE_IDS: PlatformId[] = ['antigravity', 'codex', 'github-copilot', 'windsurf'];
const TRAY_MIGRATED_PLATFORM_IDS: PlatformId[] = [
  'kiro',
  'cursor',
  'gemini',
  'codebuddy',
  'codebuddy_cn',
  'qoder',
  'trae',
];

type PersistedPlatformLayout = {
  orderedPlatformIds?: PlatformId[];
  hiddenPlatformIds?: PlatformId[];
  sidebarPlatformIds?: PlatformId[];
  trayPlatformIds?: PlatformId[];
  traySortMode?: 'auto' | 'manual';
};

interface PlatformLayoutState {
  orderedPlatformIds: PlatformId[];
  hiddenPlatformIds: PlatformId[];
  sidebarPlatformIds: PlatformId[];
  trayPlatformIds: PlatformId[];
  traySortMode: 'auto' | 'manual';

  movePlatform: (fromIndex: number, toIndex: number) => void;
  toggleHiddenPlatform: (id: PlatformId) => void;
  setHiddenPlatform: (id: PlatformId, hidden: boolean) => void;
  toggleSidebarPlatform: (id: PlatformId) => void;
  setSidebarPlatform: (id: PlatformId, enabled: boolean) => void;
  toggleTrayPlatform: (id: PlatformId) => void;
  setTrayPlatform: (id: PlatformId, enabled: boolean) => void;
  syncTrayLayout: () => void;
  resetPlatformLayout: () => void;
}

let trayLayoutSyncTimer: ReturnType<typeof setTimeout> | null = null;

function syncTrayLayoutToBackend(state: Pick<PlatformLayoutState, 'orderedPlatformIds' | 'trayPlatformIds' | 'traySortMode'>) {
  invoke('save_tray_platform_layout', {
    sortMode: state.traySortMode,
    orderedPlatformIds: state.orderedPlatformIds,
    trayPlatformIds: state.trayPlatformIds,
  }).catch((error) => {
    console.error('同步托盘平台布局失败:', error);
  });
}

function scheduleTrayLayoutSync(state: Pick<PlatformLayoutState, 'orderedPlatformIds' | 'trayPlatformIds' | 'traySortMode'>) {
  if (typeof window === 'undefined') {
    return;
  }
  if (trayLayoutSyncTimer) {
    window.clearTimeout(trayLayoutSyncTimer);
  }
  trayLayoutSyncTimer = window.setTimeout(() => {
    trayLayoutSyncTimer = null;
    syncTrayLayoutToBackend(state);
  }, 120);
}

function sanitizePlatformIds(list: unknown): PlatformId[] {
  if (!Array.isArray(list)) return [];
  const seen = new Set<PlatformId>();
  const result: PlatformId[] = [];
  for (const item of list) {
    if (typeof item !== 'string') continue;
    if (!ALL_PLATFORM_IDS.includes(item as PlatformId)) continue;
    const id = item as PlatformId;
    if (seen.has(id)) continue;
    seen.add(id);
    result.push(id);
  }
  return result;
}

function normalizeOrder(order: PlatformId[]): PlatformId[] {
  const next = sanitizePlatformIds(order);
  for (const id of ALL_PLATFORM_IDS) {
    if (!next.includes(id)) {
      next.push(id);
    }
  }
  return next;
}

function normalizeHidden(hidden: PlatformId[]): PlatformId[] {
  return sanitizePlatformIds(hidden);
}

function normalizeSidebar(sidebar: PlatformId[], hidden: PlatformId[]): PlatformId[] {
  const normalized = sanitizePlatformIds(sidebar).filter((id) => !hidden.includes(id));
  return normalized.slice(0, 2);
}

function normalizeTray(tray: PlatformId[], rawOrder: PlatformId[] = []): PlatformId[] {
  const normalized = sanitizePlatformIds(tray);
  const rawOrderSet = new Set(sanitizePlatformIds(rawOrder));
  const hasLegacyDefault = LEGACY_TRAY_CORE_IDS.every((id) => normalized.includes(id))
    && normalized.length <= ALL_PLATFORM_IDS.length - 1;

  if (!hasLegacyDefault) {
    return normalized;
  }

  const next = [...normalized];
  for (const id of TRAY_MIGRATED_PLATFORM_IDS) {
    if (next.includes(id) || rawOrderSet.has(id)) {
      continue;
    }
    next.push(id);
  }
  return next;
}

function normalizeTraySortMode(mode: unknown): 'auto' | 'manual' {
  return mode === 'manual' ? 'manual' : 'auto';
}

function loadPersistedState(): Pick<
  PlatformLayoutState,
  'orderedPlatformIds' | 'hiddenPlatformIds' | 'sidebarPlatformIds' | 'trayPlatformIds' | 'traySortMode'
> {
  try {
    const raw = localStorage.getItem(PLATFORM_LAYOUT_STORAGE_KEY);
    if (!raw) {
      return {
        orderedPlatformIds: [...ALL_PLATFORM_IDS],
        hiddenPlatformIds: [],
        sidebarPlatformIds: ['antigravity', 'codex'],
        trayPlatformIds: [...ALL_PLATFORM_IDS],
        traySortMode: 'auto',
      };
    }
    const parsed = JSON.parse(raw) as PersistedPlatformLayout;
    const hiddenPlatformIds = normalizeHidden(parsed.hiddenPlatformIds ?? []);
    const orderedPlatformIds = normalizeOrder(parsed.orderedPlatformIds ?? ALL_PLATFORM_IDS);
    const sidebarPlatformIds = normalizeSidebar(parsed.sidebarPlatformIds ?? ['antigravity', 'codex'], hiddenPlatformIds);
    const trayPlatformIds = normalizeTray(
      parsed.trayPlatformIds ?? ALL_PLATFORM_IDS,
      sanitizePlatformIds(parsed.orderedPlatformIds ?? []),
    );
    const traySortMode = normalizeTraySortMode(parsed.traySortMode);
    return {
      orderedPlatformIds,
      hiddenPlatformIds,
      sidebarPlatformIds,
      trayPlatformIds,
      traySortMode,
    };
  } catch {
    return {
      orderedPlatformIds: [...ALL_PLATFORM_IDS],
      hiddenPlatformIds: [],
      sidebarPlatformIds: ['antigravity', 'codex'],
      trayPlatformIds: [...ALL_PLATFORM_IDS],
      traySortMode: 'auto',
    };
  }
}

function persist(
  state: Pick<
    PlatformLayoutState,
    'orderedPlatformIds' | 'hiddenPlatformIds' | 'sidebarPlatformIds' | 'trayPlatformIds' | 'traySortMode'
  >,
) {
  try {
    localStorage.setItem(PLATFORM_LAYOUT_STORAGE_KEY, JSON.stringify(state));
  } catch {
    // ignore persistence failures
  }
}

export const usePlatformLayoutStore = create<PlatformLayoutState>((set, get) => ({
  ...loadPersistedState(),

  movePlatform: (fromIndex, toIndex) => {
    const current = [...get().orderedPlatformIds];
    if (fromIndex < 0 || toIndex < 0 || fromIndex >= current.length || toIndex >= current.length) return;
    if (fromIndex === toIndex) return;
    const [item] = current.splice(fromIndex, 1);
    current.splice(toIndex, 0, item);
    const orderedPlatformIds = normalizeOrder(current);
    const next = {
      orderedPlatformIds,
      hiddenPlatformIds: [...get().hiddenPlatformIds],
      sidebarPlatformIds: [...get().sidebarPlatformIds],
      trayPlatformIds: [...get().trayPlatformIds],
      traySortMode: 'manual' as const,
    };
    set(next);
    persist(next);
    scheduleTrayLayoutSync(next);
  },

  toggleHiddenPlatform: (id) => {
    const hidden = [...get().hiddenPlatformIds];
    const exists = hidden.includes(id);
    const nextHidden = exists ? hidden.filter((item) => item !== id) : [...hidden, id];
    const hiddenPlatformIds = normalizeHidden(nextHidden);
    const sidebarPlatformIds = normalizeSidebar(get().sidebarPlatformIds, hiddenPlatformIds);
    const next = {
      orderedPlatformIds: [...get().orderedPlatformIds],
      hiddenPlatformIds,
      sidebarPlatformIds,
      trayPlatformIds: [...get().trayPlatformIds],
      traySortMode: get().traySortMode,
    };
    set(next);
    persist(next);
  },

  setHiddenPlatform: (id, hidden) => {
    const current = get().hiddenPlatformIds;
    const has = current.includes(id);
    if ((hidden && has) || (!hidden && !has)) return;
    get().toggleHiddenPlatform(id);
  },

  toggleSidebarPlatform: (id) => {
    const hiddenPlatformIds = [...get().hiddenPlatformIds];
    if (hiddenPlatformIds.includes(id)) return;

    const current = [...get().sidebarPlatformIds];
    let nextSidebar: PlatformId[] = [];

    if (current.includes(id)) {
      nextSidebar = current.filter((item) => item !== id);
    } else if (current.length < 2) {
      nextSidebar = [...current, id];
    } else {
      return;
    }

    const sidebarPlatformIds = normalizeSidebar(nextSidebar, hiddenPlatformIds);
    const next = {
      orderedPlatformIds: [...get().orderedPlatformIds],
      hiddenPlatformIds,
      sidebarPlatformIds,
      trayPlatformIds: [...get().trayPlatformIds],
      traySortMode: get().traySortMode,
    };
    set(next);
    persist(next);
  },

  setSidebarPlatform: (id, enabled) => {
    const current = get().sidebarPlatformIds.includes(id);
    if (current === enabled) return;
    get().toggleSidebarPlatform(id);
  },

  toggleTrayPlatform: (id) => {
    const current = [...get().trayPlatformIds];
    const exists = current.includes(id);
    const nextTray = exists
      ? current.filter((item) => item !== id)
      : [...current, id];

    const next = {
      orderedPlatformIds: [...get().orderedPlatformIds],
      hiddenPlatformIds: [...get().hiddenPlatformIds],
      sidebarPlatformIds: [...get().sidebarPlatformIds],
      trayPlatformIds: normalizeTray(nextTray),
      traySortMode: get().traySortMode,
    };
    set(next);
    persist(next);
    scheduleTrayLayoutSync(next);
  },

  setTrayPlatform: (id, enabled) => {
    const current = get().trayPlatformIds.includes(id);
    if (current === enabled) return;
    get().toggleTrayPlatform(id);
  },

  syncTrayLayout: () => {
    const state = get();
    syncTrayLayoutToBackend({
      orderedPlatformIds: state.orderedPlatformIds,
      trayPlatformIds: state.trayPlatformIds,
      traySortMode: state.traySortMode,
    });
  },

  resetPlatformLayout: () => {
    const next = {
      orderedPlatformIds: [...ALL_PLATFORM_IDS],
      hiddenPlatformIds: [],
      sidebarPlatformIds: ['antigravity', 'codex'] as PlatformId[],
      trayPlatformIds: [...ALL_PLATFORM_IDS],
      traySortMode: 'auto' as const,
    };
    set(next);
    persist(next);
    scheduleTrayLayoutSync(next);
  },
}));

if (typeof window !== 'undefined') {
  window.setTimeout(() => {
    usePlatformLayoutStore.getState().syncTrayLayout();
  }, 0);
}
