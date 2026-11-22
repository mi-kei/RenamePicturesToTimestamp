import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { JobReport, Progress } from "../types";

export function onJobProgress(handler: (progress: Progress) => void): Promise<UnlistenFn> {
  return listen<Progress>("job-progress", (event) => handler(event.payload));
}

export function onJobFinished(handler: (report: JobReport) => void): Promise<UnlistenFn> {
  return listen<JobReport>("job-finished", (event) => handler(event.payload));
}
