import { create } from "zustand";
import {
  buildPlan,
  clearSession,
  getSummary,
  readMetadata,
  scanPaths,
  setOverride,
} from "../api/commands";
import type { PlanOptions, PlanSummary, Row, StatusKind } from "../types";

export const DEFAULT_TEMPLATE = "IMG_{date:YYYYMMDD_HHmmss}_{name}";
const PLAN_DELAY_MS = 200;

export type BusyPhase = "idle" | "scan" | "exif" | "plan";

export function errorMessage(reason: unknown): string {
  if (typeof reason === "string") return reason;
  if (reason instanceof Error) return reason.message;
  return String(reason);
}

interface SessionStore {
  template: string;
  options: PlanOptions;
  recursive: boolean;
  summary: PlanSummary | null;
  templateError: string | null;
  notice: string | null;
  warnings: string[];
  busy: BusyPhase;
  filter: StatusKind[] | null;
  rowsVersion: number;
  selectedId: number | null;
  selectedRow: Row | null;
  addPaths: (paths: string[]) => Promise<void>;
  setTemplate: (template: string) => void;
  setOptions: (patch: Partial<PlanOptions>) => void;
  setRecursive: (recursive: boolean) => void;
  setFilter: (filter: StatusKind[] | null) => void;
  select: (row: Row | null) => void;
  setExcluded: (id: number, excluded: boolean) => Promise<void>;
  setManualName: (id: number, name: string) => Promise<void>;
  replan: () => Promise<void>;
  refresh: () => Promise<void>;
  clear: () => Promise<void>;
  dismissNotice: () => void;
}

let planTimer: ReturnType<typeof setTimeout> | undefined;

export const useSessionStore = create<SessionStore>((set, get) => ({
  template: DEFAULT_TEMPLATE,
  options: { onConflict: "suffix", extCase: "keep", skipAlreadyNamed: true },
  recursive: true,
  summary: null,
  templateError: null,
  notice: null,
  warnings: [],
  busy: "idle",
  filter: null,
  rowsVersion: 0,
  selectedId: null,
  selectedRow: null,

  addPaths: async (paths) => {
    if (paths.length === 0 || get().busy !== "idle") return;
    set({ busy: "scan", notice: null });
    try {
      const scanned = await scanPaths(paths, get().recursive);
      const warnings = [...scanned.errors];
      if (scanned.added > 0) {
        set({ busy: "exif" });
        const metadata = await readMetadata();
        if (metadata.warning) warnings.push(metadata.warning);
      }
      set({
        warnings,
        notice:
          scanned.added > 0
            ? `${scanned.added.toLocaleString()} 件のファイルを追加しました`
            : "追加できる写真ファイルが見つかりませんでした",
      });
      set({ busy: "plan" });
      await get().replan();
    } catch (reason) {
      set({ notice: `読み込みに失敗しました: ${errorMessage(reason)}` });
    } finally {
      set({ busy: "idle" });
    }
  },

  setTemplate: (template) => {
    set({ template });
    clearTimeout(planTimer);
    planTimer = setTimeout(() => void get().replan(), PLAN_DELAY_MS);
  },

  setOptions: (patch) => {
    set({ options: { ...get().options, ...patch } });
    clearTimeout(planTimer);
    void get().replan();
  },

  setRecursive: (recursive) => set({ recursive }),

  setFilter: (filter) => set({ filter, rowsVersion: get().rowsVersion + 1 }),

  select: (row) => set({ selectedId: row?.id ?? null, selectedRow: row }),

  setExcluded: async (id, excluded) => {
    try {
      const summary = await setOverride(id, { excluded });
      set({ summary, rowsVersion: get().rowsVersion + 1 });
    } catch (reason) {
      set({ notice: errorMessage(reason) });
    }
  },

  setManualName: async (id, name) => {
    try {
      const summary = await setOverride(id, { manualName: name });
      set({ summary, rowsVersion: get().rowsVersion + 1 });
    } catch (reason) {
      set({ notice: errorMessage(reason) });
    }
  },

  replan: async () => {
    try {
      const summary = await buildPlan(get().template, get().options);
      set({ summary, templateError: null, rowsVersion: get().rowsVersion + 1 });
    } catch (reason) {
      set({ templateError: errorMessage(reason) });
    }
  },

  refresh: async () => {
    try {
      const summary = await getSummary();
      set({ summary, rowsVersion: get().rowsVersion + 1 });
    } catch (reason) {
      set({ notice: errorMessage(reason) });
    }
  },

  clear: async () => {
    await clearSession();
    set({
      summary: null,
      warnings: [],
      notice: null,
      selectedId: null,
      selectedRow: null,
      rowsVersion: get().rowsVersion + 1,
    });
  },

  dismissNotice: () => set({ notice: null, warnings: [] }),
}));
