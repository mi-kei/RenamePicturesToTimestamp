import { STATUS_LABELS } from "../status";
import { useSessionStore } from "../store/sessionStore";

const SOURCE_LABELS = {
  native: "内蔵リーダー（kamadak-exif）",
  exiftool: "ExifTool（補助）",
} as const;

export function ExifPanel() {
  const row = useSessionStore((state) => state.selectedRow);
  const setExcluded = useSessionStore((state) => state.setExcluded);
  const setManualName = useSessionStore((state) => state.setManualName);

  if (!row) {
    return (
      <aside className="exif-panel empty">
        <p>一覧の行を選ぶと、撮影日時や機種などの情報をここに表示します。</p>
      </aside>
    );
  }

  const fields: [string, string | null][] = [
    ["元の名前", row.originalName],
    ["新しい名前", row.newName],
    ["フォルダ", row.directory],
    ["撮影日時", row.takenAt],
    ["メーカー", row.make],
    ["機種", row.model],
    ["読み取り方法", row.source ? SOURCE_LABELS[row.source] : null],
  ];

  return (
    <aside className="exif-panel" aria-label="選択中のファイルの情報">
      <div className="panel-heading">
        <h2>ファイル情報</h2>
        <span className={`status-badge status-${row.status.kind}`}>{STATUS_LABELS[row.status.kind]}</span>
      </div>
      {row.status.detail && <p className="status-detail">{row.status.detail}</p>}
      <dl>
        {fields.map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd className={value ? undefined : "missing"}>{value ?? "—"}</dd>
          </div>
        ))}
      </dl>
      {row.exifError && <p className="status-detail">EXIF: {row.exifError}</p>}
      <div className="panel-actions">
        {row.status.kind !== "done" && (
          <button type="button" className="ghost" onClick={() => void setExcluded(row.id, !row.excluded)}>
            {row.excluded ? "対象に戻す" : "対象から外す"}
          </button>
        )}
        {row.manual && (
          <button type="button" className="ghost" onClick={() => void setManualName(row.id, "")}>
            手動の名前を解除
          </button>
        )}
      </div>
    </aside>
  );
}
