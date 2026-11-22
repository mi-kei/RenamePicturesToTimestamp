import type { StatusKind } from "./types";

export const STATUS_LABELS: Record<StatusKind, string> = {
  pending: "未判定",
  ok: "OK",
  suffixed: "連番付与",
  conflict: "衝突",
  noTimestamp: "日時なし",
  unchanged: "変更なし",
  excluded: "除外",
  error: "エラー",
  done: "完了",
};

export interface StatusFilterOption {
  key: string;
  label: string;
  kinds: StatusKind[] | null;
}

export const STATUS_FILTERS: StatusFilterOption[] = [
  { key: "all", label: "すべて", kinds: null },
  { key: "ok", label: "OK", kinds: ["ok", "suffixed"] },
  { key: "conflict", label: "衝突", kinds: ["conflict"] },
  { key: "noTimestamp", label: "日時なし", kinds: ["noTimestamp"] },
  { key: "unchanged", label: "変更なし", kinds: ["unchanged"] },
  { key: "excluded", label: "除外", kinds: ["excluded"] },
  { key: "error", label: "エラー", kinds: ["error"] },
  { key: "done", label: "完了", kinds: ["done"] },
];
