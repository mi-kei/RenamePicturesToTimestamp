import { create } from "zustand";
import {
  cancelJob,
  clearJournals as clearJournalsCommand,
  deleteJournal as deleteJournalCommand,
  executeRename,
  getStorageInfo,
  listJournals,
  openStorageDir as openStorageDirCommand,
  resumeJob,
  undoJob,
} from "../api/commands";
import { onJobFinished, onJobProgress } from "../api/events";
import type { JobKind, JobReport, JournalInfo, LogLine, Progress, StorageInfo } from "../types";
import { errorMessage, useSessionStore } from "./sessionStore";

const MAX_LOG_LINES = 5000;
const ROW_REFRESH_INTERVAL_MS = 1000;
let lastRowRefresh = 0;

interface RunningJob {
  id: string;
  kind: JobKind;
  skipped: number;
}

interface JobStore {
  running: RunningJob | null;
  progress: Progress | null;
  logs: LogLine[];
  lastReport: JobReport | null;
  lastSkipped: number;
  journals: JournalInfo[];
  storage: StorageInfo | null;
  historyOpen: boolean;
  historyError: string | null;
  interrupted: JournalInfo | null;
  error: string | null;
  init: () => () => void;
  execute: () => Promise<void>;
  undo: (job: string) => Promise<void>;
  resume: (job: string) => Promise<void>;
  cancel: () => Promise<void>;
  loadJournals: () => Promise<JournalInfo[]>;
  loadStorageInfo: () => Promise<void>;
  deleteJournal: (job: string) => Promise<void>;
  clearJournals: () => Promise<void>;
  openStorageDir: () => Promise<void>;
  setHistoryOpen: (open: boolean) => void;
  dismissInterrupted: () => void;
  dismissReport: () => void;
}

export const useJobStore = create<JobStore>((set, get) => {
  const begin = async (kind: JobKind, skipped: number, start: () => Promise<string>) => {
    if (get().running) return;
    set({ error: null, lastReport: null, progress: null, logs: [], interrupted: null });
    try {
      const id = await start();
      // Very small jobs can finish before the command returns.
      if (get().lastReport?.job !== id) set({ running: { id, kind, skipped } });
      set({ lastSkipped: skipped });
    } catch (reason) {
      set({ error: errorMessage(reason) });
    }
  };

  return {
    running: null,
    progress: null,
    logs: [],
    lastReport: null,
    lastSkipped: 0,
    journals: [],
    storage: null,
    historyOpen: false,
    historyError: null,
    interrupted: null,
    error: null,

    init: () => {
      let disposed = false;
      const stops: (() => void)[] = [];
      const keep = (promise: Promise<() => void>) =>
        promise
          .then((stop) => (disposed ? stop() : stops.push(stop)))
          .catch(() => {
            // Events are unavailable outside of Tauri.
          });

      keep(
        onJobProgress((progress) => {
          if (progress.phase !== "rename" && progress.phase !== "undo") {
            set({ progress });
            return;
          }
          const logs = progress.recent.length > 0 ? [...get().logs, ...progress.recent] : get().logs;
          set({
            progress,
            logs: logs.length > MAX_LOG_LINES ? logs.slice(logs.length - MAX_LOG_LINES) : logs,
          });
          // Let the preview rows follow the job (statuses turn into "done" as files are renamed).
          const now = Date.now();
          if (now - lastRowRefresh >= ROW_REFRESH_INTERVAL_MS) {
            lastRowRefresh = now;
            void useSessionStore.getState().refresh();
          }
        }),
      );
      keep(
        onJobFinished((report) => {
          set({ running: null, lastReport: report });
          void useSessionStore.getState().refresh();
          void get().loadJournals();
        }),
      );

      void get().loadStorageInfo();
      void get()
        .loadJournals()
        .then((journals) => {
          const interrupted = journals.find((journal) => journal.incomplete && !journal.undoneBy) ?? null;
          if (!disposed) set({ interrupted });
        });

      return () => {
        disposed = true;
        stops.forEach((stop) => stop());
      };
    },

    execute: async () => {
      const summary = useSessionStore.getState().summary;
      const skipped = summary ? summary.total - summary.ok - summary.done : 0;
      await begin("rename", skipped, executeRename);
    },

    undo: async (job) => {
      set({ historyOpen: false });
      await begin("undo", 0, () => undoJob(job));
    },

    resume: async (job) => {
      set({ historyOpen: false });
      await begin("rename", 0, () => resumeJob(job));
    },

    cancel: async () => {
      try {
        await cancelJob();
      } catch (reason) {
        set({ error: errorMessage(reason) });
      }
    },

    loadJournals: async () => {
      try {
        const journals = await listJournals();
        set({ journals });
        return journals;
      } catch (reason) {
        set({ error: errorMessage(reason) });
        return [];
      }
    },

    loadStorageInfo: async () => {
      try {
        const storage = await getStorageInfo();
        set({ storage });
      } catch {
        // Storage info is unavailable outside of Tauri.
      }
    },

    deleteJournal: async (job) => {
      set({ historyError: null });
      try {
        await deleteJournalCommand(job);
        const interrupted = get().interrupted;
        if (interrupted?.job === job) set({ interrupted: null });
        await get().loadJournals();
      } catch (reason) {
        set({ historyError: errorMessage(reason) });
      }
    },

    clearJournals: async () => {
      set({ historyError: null });
      try {
        await clearJournalsCommand();
        set({ interrupted: null });
        await get().loadJournals();
      } catch (reason) {
        set({ historyError: errorMessage(reason) });
      }
    },

    openStorageDir: async () => {
      set({ historyError: null });
      try {
        await openStorageDirCommand();
      } catch (reason) {
        set({ historyError: errorMessage(reason) });
      }
    },

    setHistoryOpen: (historyOpen) => {
      set({ historyOpen, historyError: null });
      if (historyOpen) {
        void get().loadJournals();
        void get().loadStorageInfo();
      }
    },

    dismissInterrupted: () => set({ interrupted: null }),

    dismissReport: () => set({ lastReport: null, progress: null, logs: [], error: null }),
  };
});
