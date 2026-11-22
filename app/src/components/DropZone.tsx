import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useSessionStore } from "../store/sessionStore";

/** Turns the whole window into a drop target for files and folders. */
export function DropZone() {
  const addPaths = useSessionStore((state) => state.addPaths);
  const [active, setActive] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setActive(true);
        } else if (payload.type === "leave") {
          setActive(false);
        } else if (payload.type === "drop") {
          setActive(false);
          void addPaths(payload.paths);
        }
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Outside of Tauri (e.g. plain browser preview) drag & drop is unavailable.
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [addPaths]);

  if (!active) return null;
  return (
    <div className="drop-overlay" aria-hidden="true">
      <div className="drop-overlay-card">
        <span className="drop-icon">＋</span>
        <strong>ここにドロップして追加</strong>
        <span>ファイルとフォルダを混ぜてドロップできます</span>
      </div>
    </div>
  );
}

export function EmptyState() {
  const recursive = useSessionStore((state) => state.recursive);
  return (
    <div className="empty-state">
      <span className="drop-icon" aria-hidden="true">＋</span>
      <strong>写真やフォルダをウィンドウにドロップ</strong>
      <span>
        {recursive ? "サブフォルダも含めて" : "フォルダの直下だけを"}
        読み込み、撮影日時から新しい名前を作ります。実行するまでファイルは変更されません。
      </span>
    </div>
  );
}
