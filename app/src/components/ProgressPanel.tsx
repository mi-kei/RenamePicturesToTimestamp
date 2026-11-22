import { useJobStore } from "../store/jobStore";
import { LogView } from "./LogView";

const STATE_LABELS: Record<string, string> = {
  completed: "完了しました",
  cancelled: "キャンセルしました",
  failed: "履歴の書き込みに失敗したため中断しました",
};

function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  const rounded = Math.ceil(seconds);
  const minutes = Math.floor(rounded / 60);
  const rest = rounded % 60;
  return minutes > 0 ? `${minutes}分${rest.toString().padStart(2, "0")}秒` : `${rest}秒`;
}

export function ProgressPanel() {
  const running = useJobStore((state) => state.running);
  const progress = useJobStore((state) => state.progress);
  const report = useJobStore((state) => state.lastReport);
  const skipped = useJobStore((state) => state.lastSkipped);
  const error = useJobStore((state) => state.error);
  const cancel = useJobStore((state) => state.cancel);
  const dismiss = useJobStore((state) => state.dismissReport);

  const jobProgress = progress && (progress.phase === "rename" || progress.phase === "undo") ? progress : null;
  if (!running && !report && !error) return null;

  const kind = running?.kind ?? report?.kind ?? "rename";
  const total = jobProgress?.total ?? report?.total ?? 0;
  const done = jobProgress?.done ?? report?.total ?? 0;
  const ok = jobProgress?.ok ?? report?.ok ?? 0;
  const errors = jobProgress?.errors ?? report?.failed ?? 0;
  const rate = jobProgress?.rate ?? 0;
  const remaining = rate > 0 ? (total - done) / rate : Number.NaN;
  const percent = total > 0 ? Math.round((done / total) * 100) : 0;

  return (
    <section className="progress-panel" aria-live="polite">
      <div className="progress-head">
        <strong>
          {kind === "undo" ? "元に戻しています" : "リネームしています"}
          {report && `：${STATE_LABELS[report.state] ?? report.state}`}
        </strong>
        <span className="progress-count">
          {done.toLocaleString()} / {total.toLocaleString()}（{percent}%）
        </span>
        {running ? (
          <button type="button" className="danger" onClick={() => void cancel()}>
            キャンセル
          </button>
        ) : (
          <button type="button" className="ghost" onClick={dismiss}>
            閉じる
          </button>
        )}
      </div>
      <progress value={done} max={total || 1} />
      <div className="progress-stats">
        <span className="stat ok">完了 {ok.toLocaleString()}</span>
        {kind === "rename" && <span className="stat skipped">スキップ {skipped.toLocaleString()}</span>}
        <span className="stat error">エラー {errors.toLocaleString()}</span>
        {running && (
          <>
            <span className="stat">{rate.toFixed(0)} 件/秒</span>
            <span className="stat">残り 約{formatDuration(remaining)}</span>
          </>
        )}
        {running && jobProgress?.current && <span className="current">処理中: {jobProgress.current}</span>}
      </div>
      {(error || report?.error) && (
        <p className="field-error" role="alert">
          {error ?? report?.error}
        </p>
      )}
      <LogView />
    </section>
  );
}
