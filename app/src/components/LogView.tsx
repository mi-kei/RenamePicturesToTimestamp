import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useJobStore } from "../store/jobStore";

const LINE_HEIGHT = 22;

export function LogView() {
  const logs = useJobStore((state) => state.logs);
  const scrollElement = useRef<HTMLDivElement>(null);
  const followTail = useRef(true);
  const virtualizer = useVirtualizer({
    count: logs.length,
    getScrollElement: () => scrollElement.current,
    estimateSize: () => LINE_HEIGHT,
    overscan: 20,
  });

  useEffect(() => {
    if (followTail.current && logs.length > 0) virtualizer.scrollToIndex(logs.length - 1, { align: "end" });
  }, [logs.length, virtualizer]);

  const onScroll = () => {
    const element = scrollElement.current;
    if (!element) return;
    followTail.current = element.scrollHeight - element.scrollTop - element.clientHeight < LINE_HEIGHT * 2;
  };

  return (
    <div className="log-view" ref={scrollElement} onScroll={onScroll} aria-label="処理のログ">
      {logs.length === 0 && <p className="log-empty">ログはまだありません</p>}
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((item) => {
          const line = logs[item.index];
          return (
            <div
              key={item.key}
              className={`log-line log-${line.level}`}
              style={{ transform: `translateY(${item.start}px)`, height: LINE_HEIGHT }}
            >
              <span className="log-time">{line.timestamp}</span>
              <span className="log-message">{line.message}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
