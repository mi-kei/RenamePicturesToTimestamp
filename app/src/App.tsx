import { useEffect, useState } from "react";
import { DropZone, EmptyState } from "./components/DropZone";
import { ExifPanel } from "./components/ExifPanel";
import { ConfirmDialog, HistoryDialog, InterruptedDialog } from "./components/HistoryDialog";
import { PreviewTable, StatusFilterBar } from "./components/PreviewTable";
import { ProgressPanel } from "./components/ProgressPanel";
import { TemplateEditor } from "./components/TemplateEditor";
import { useJobStore } from "./store/jobStore";
import { useSessionStore, type BusyPhase } from "./store/sessionStore";

const BUSY_LABELS: Record<BusyPhase, string> = {
  idle: "",
  scan: "ファイルを探しています",
  exif: "撮影日時を読み取っています",
  plan: "新しい名前を決めています",
};

function BusyIndicator() {
  const busy = useSessionStore((state) => state.busy);
  const progress = useJobStore((state) => state.progress);
  if (busy === "idle") return null;
  const matching = progress && progress.phase === busy ? progress : null;
  const count = matching
    ? matching.total > 0
      ? `（${matching.done.toLocaleString()} / ${matching.total.toLocaleString()}）`
      : `（${matching.done.toLocaleString()} 件）`
    : "";
  return (
    <span className="busy">
      {BUSY_LABELS[busy]}…{count}
    </span>
  );
}

export default function App() {
  const summary = useSessionStore((state) => state.summary);
  const busy = useSessionStore((state) => state.busy);
  const notice = useSessionStore((state) => state.notice);
  const warnings = useSessionStore((state) => state.warnings);
  const dismissNotice = useSessionStore((state) => state.dismissNotice);
  const clear = useSessionStore((state) => state.clear);
  const running = useJobStore((state) => state.running);
  const init = useJobStore((state) => state.init);
  const execute = useJobStore((state) => state.execute);
  const setHistoryOpen = useJobStore((state) => state.setHistoryOpen);
  const [confirming, setConfirming] = useState(false);

  useEffect(() => init(), [init]);

  const hasItems = (summary?.total ?? 0) > 0;
  const renamable = summary?.ok ?? 0;
  const untouched = summary ? summary.total - summary.ok - summary.done : 0;
  const locked = busy !== "idle" || running !== null;

  return (
    <main className="app-shell">
      <DropZone />
      <header className="topbar">
        <div className="brand-mark" aria-hidden="true">
          PR
        </div>
        <div>
          <p className="eyebrow">PHOTO RENAMER</p>
          <h1>撮影日時でファイル名を整理</h1>
        </div>
        <div className="topbar-actions">
          <BusyIndicator />
          <button type="button" className="ghost" onClick={() => setHistoryOpen(true)}>
            履歴
          </button>
          <button type="button" className="ghost" disabled={!hasItems || locked} onClick={() => void clear()}>
            一覧をクリア
          </button>
          <button type="button" disabled={renamable === 0 || locked} onClick={() => setConfirming(true)}>
            {renamable.toLocaleString()} 件をリネーム
          </button>
        </div>
      </header>

      {(notice || warnings.length > 0) && (
        <div className="notice" role="status">
          <div>
            {notice && <p>{notice}</p>}
            {warnings.slice(0, 5).map((warning) => (
              <p key={warning} className="warning">
                {warning}
              </p>
            ))}
            {warnings.length > 5 && <p className="warning">ほか {warnings.length - 5} 件の警告があります</p>}
          </div>
          <button type="button" className="ghost" onClick={dismissNotice} aria-label="お知らせを閉じる">
            ×
          </button>
        </div>
      )}

      <fieldset className="plain" disabled={running !== null}>
        <TemplateEditor />
      </fieldset>

      <div className="workspace">
        <fieldset className="plain list-area" disabled={running !== null} aria-label="プレビュー">
          {hasItems ? (
            <>
              <StatusFilterBar />
              <PreviewTable />
            </>
          ) : (
            <EmptyState />
          )}
        </fieldset>
        <ExifPanel />
      </div>

      <ProgressPanel />
      <HistoryDialog />
      <InterruptedDialog />

      {confirming && (
        <ConfirmDialog
          title="実行前の確認"
          message={
            <>
              <p>{renamable.toLocaleString()} 件のファイル名を変更します。</p>
              <p>
                対象外・衝突・日時なしなどの {untouched.toLocaleString()} 件は変更しません。
                実行内容は履歴に保存され、あとから元に戻せます。
              </p>
            </>
          }
          confirmLabel="実行する"
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            void execute();
          }}
        />
      )}
    </main>
  );
}
