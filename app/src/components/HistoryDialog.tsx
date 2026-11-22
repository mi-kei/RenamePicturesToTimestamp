import { useState, type ReactNode } from "react";
import { useJobStore } from "../store/jobStore";
import type { JournalInfo } from "../types";

const STATE_LABELS: Record<string, string> = {
  completed: "完了",
  cancelled: "キャンセル",
  interrupted: "途中で終了",
  failed: "失敗",
  running: "実行中",
};

function formatCreated(created: string): string {
  return created.replace("T", " ");
}

function canResume(journal: JournalInfo): boolean {
  return !journal.undoneBy && journal.state !== "running" && journal.done + journal.failed < journal.count;
}

function canUndo(journal: JournalInfo): boolean {
  return journal.kind === "rename" && !journal.undoneBy && journal.state !== "running" && journal.done > 0;
}

export function Modal({ title, children, onClose }: { title: string; children: ReactNode; onClose?: () => void }) {
  return (
    <div className="modal-backdrop" role="presentation" onClick={onClose}>
      <div className="modal" role="dialog" aria-modal="true" aria-label={title} onClick={(event) => event.stopPropagation()}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  danger,
  onConfirm,
  onCancel,
}: {
  title: string;
  message: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Modal title={title} onClose={onCancel}>
      <div className="modal-message">{message}</div>
      <div className="modal-actions">
        <button type="button" className="ghost" onClick={onCancel}>
          やめる
        </button>
        <button type="button" className={danger ? "danger" : undefined} onClick={onConfirm} autoFocus>
          {confirmLabel}
        </button>
      </div>
    </Modal>
  );
}

export function HistoryDialog() {
  const open = useJobStore((state) => state.historyOpen);
  const journals = useJobStore((state) => state.journals);
  const running = useJobStore((state) => state.running);
  const setOpen = useJobStore((state) => state.setHistoryOpen);
  const undo = useJobStore((state) => state.undo);
  const resume = useJobStore((state) => state.resume);
  const [confirmUndo, setConfirmUndo] = useState<JournalInfo | null>(null);

  if (!open) return null;

  if (confirmUndo) {
    return (
      <ConfirmDialog
        title="元に戻す"
        message={
          <p>
            {formatCreated(confirmUndo.created)} の実行で変更した {confirmUndo.done.toLocaleString()} 件を元の名前に戻します。
            元の名前に別のファイルがある場合は、上書きせずにエラーとして記録します。
          </p>
        }
        confirmLabel="元に戻す"
        danger
        onCancel={() => setConfirmUndo(null)}
        onConfirm={() => {
          setConfirmUndo(null);
          void undo(confirmUndo.job);
        }}
      />
    );
  }

  return (
    <Modal title="実行の履歴" onClose={() => setOpen(false)}>
      {journals.length === 0 ? (
        <p className="modal-message">まだ履歴はありません。</p>
      ) : (
        <div className="history-list">
          {journals.map((journal) => (
            <div key={journal.job} className="history-item">
              <div className="history-main">
                <strong>
                  {journal.kind === "undo" ? "元に戻す" : "リネーム"}・{formatCreated(journal.created)}
                </strong>
                <span className="history-meta">
                  {journal.done.toLocaleString()} / {journal.count.toLocaleString()} 件
                  {journal.failed > 0 && `・エラー ${journal.failed.toLocaleString()} 件`}
                  {journal.kind === "rename" && journal.template && `・${journal.template}`}
                </span>
              </div>
              <span className={`history-state state-${journal.undoneBy ? "undone" : journal.state}`}>
                {journal.undoneBy ? "元に戻し済み" : STATE_LABELS[journal.state] ?? journal.state}
              </span>
              <div className="history-actions">
                {canResume(journal) && (
                  <button type="button" className="ghost" disabled={!!running} onClick={() => void resume(journal.job)}>
                    続きから再開
                  </button>
                )}
                {canUndo(journal) && (
                  <button type="button" className="ghost" disabled={!!running} onClick={() => setConfirmUndo(journal)}>
                    元に戻す
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
      <div className="modal-actions">
        <button type="button" className="ghost" onClick={() => setOpen(false)}>
          閉じる
        </button>
      </div>
    </Modal>
  );
}

export function InterruptedDialog() {
  const interrupted = useJobStore((state) => state.interrupted);
  const running = useJobStore((state) => state.running);
  const dismiss = useJobStore((state) => state.dismissInterrupted);
  const undo = useJobStore((state) => state.undo);
  const resume = useJobStore((state) => state.resume);

  if (!interrupted || running) return null;
  const remaining = interrupted.count - interrupted.done - interrupted.failed;
  const label = interrupted.kind === "undo" ? "「元に戻す」処理" : "リネーム";

  return (
    <Modal title="前回の処理が途中で終わっています">
      <div className="modal-message">
        <p>
          {formatCreated(interrupted.created)} に始めた{label}は、{interrupted.count.toLocaleString()} 件中{" "}
          {interrupted.done.toLocaleString()} 件まで進んだところで終わっています（残り {remaining.toLocaleString()} 件）。
        </p>
        <p>続きから再開するか、ここまでの変更を元に戻すかを選んでください。</p>
      </div>
      <div className="modal-actions">
        <button type="button" className="ghost" onClick={dismiss}>
          あとで決める
        </button>
        {interrupted.kind === "rename" && interrupted.done > 0 && (
          <button type="button" className="danger" onClick={() => void undo(interrupted.job)}>
            元に戻す
          </button>
        )}
        <button type="button" onClick={() => void resume(interrupted.job)}>
          続きから再開
        </button>
      </div>
    </Modal>
  );
}
