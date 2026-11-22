import { invoke } from "@tauri-apps/api/core";
import type {
  JournalInfo,
  MetadataSummary,
  PlanOptions,
  PlanSummary,
  RowPage,
  ScanSummary,
  StatusKind,
  StorageInfo,
} from "../types";

export function scanPaths(paths: string[], recursive: boolean, extensions: string[] = []): Promise<ScanSummary> {
  return invoke<ScanSummary>("scan_paths", { paths, recursive, extensions });
}

export function readMetadata(): Promise<MetadataSummary> {
  return invoke<MetadataSummary>("read_metadata");
}

export function buildPlan(template: string, options: PlanOptions): Promise<PlanSummary> {
  return invoke<PlanSummary>("build_plan", { template, options });
}

export function getRows(offset: number, limit: number, filter: StatusKind[] | null): Promise<RowPage> {
  return invoke<RowPage>("get_rows", { offset, limit, filter });
}

export function setOverride(
  id: number,
  change: { excluded?: boolean; manualName?: string },
): Promise<PlanSummary> {
  return invoke<PlanSummary>("set_override", {
    id,
    excluded: change.excluded ?? null,
    manualName: change.manualName ?? null,
  });
}

export function getSummary(): Promise<PlanSummary> {
  return invoke<PlanSummary>("get_summary");
}

export function clearSession(): Promise<void> {
  return invoke<void>("clear_session");
}

export function executeRename(): Promise<string> {
  return invoke<string>("execute_rename");
}

export function cancelJob(): Promise<boolean> {
  return invoke<boolean>("cancel_job");
}

export function listJournals(): Promise<JournalInfo[]> {
  return invoke<JournalInfo[]>("list_journals");
}

export function getStorageInfo(): Promise<StorageInfo> {
  return invoke<StorageInfo>("get_storage_info");
}

export function deleteJournal(job: string): Promise<void> {
  return invoke<void>("delete_journal", { job });
}

export function clearJournals(): Promise<number> {
  return invoke<number>("clear_journals");
}

export function openStorageDir(): Promise<void> {
  return invoke<void>("open_storage_dir");
}

export function undoJob(job: string): Promise<string> {
  return invoke<string>("undo_job", { job });
}

export function resumeJob(job: string): Promise<string> {
  return invoke<string>("resume_job", { job });
}
