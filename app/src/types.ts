export type ProgressPhase = "scan" | "exif" | "rename" | "undo";

export interface LogLine {
  timestamp: string;
  level: "info" | "warning" | "error";
  message: string;
}

export interface Progress {
  phase: ProgressPhase;
  done: number;
  total: number;
  current: string | null;
  ok: number;
  skipped: number;
  errors: number;
  rate: number;
  recent: LogLine[];
}

export type JobKind = "rename" | "undo";

export interface JobReport {
  job: string;
  kind: JobKind;
  state: string;
  total: number;
  ok: number;
  failed: number;
  error: string | null;
}

export interface JournalInfo {
  job: string;
  kind: JobKind;
  created: string;
  template: string;
  count: number;
  done: number;
  failed: number;
  state: string;
  incomplete: boolean;
  undoOf: string | null;
  undoneBy: string | null;
}

export interface ScannedFile {
  id: number;
  path: string;
}

export interface ScanSummary {
  found: number;
  added: number;
  skipped: number;
  files: ScannedFile[];
  errors: string[];
}

export type StatusKind =
  | "pending"
  | "ok"
  | "suffixed"
  | "conflict"
  | "noTimestamp"
  | "unchanged"
  | "excluded"
  | "error"
  | "done";

export interface ItemStatus {
  kind: StatusKind;
  detail?: string;
}

export type ConflictResolution = "suffix" | "skip";
export type ExtCase = "keep" | "lower" | "upper";

export interface PlanOptions {
  onConflict: ConflictResolution;
  extCase: ExtCase;
  skipAlreadyNamed: boolean;
}

export interface PlanSummary {
  total: number;
  ok: number;
  suffixed: number;
  conflict: number;
  noTimestamp: number;
  unchanged: number;
  excluded: number;
  errors: number;
  done: number;
}

export interface Row {
  id: number;
  originalName: string;
  directory: string;
  newName: string | null;
  takenAt: string | null;
  status: ItemStatus;
  excluded: boolean;
  manual: boolean;
  make: string | null;
  model: string | null;
  source: "native" | "exiftool" | null;
  exifError: string | null;
}

export interface RowPage {
  total: number;
  offset: number;
  rows: Row[];
}

export interface MetadataSummary {
  total: number;
  withTimestamp: number;
  noTimestamp: number;
  errors: number;
  warning: string | null;
}