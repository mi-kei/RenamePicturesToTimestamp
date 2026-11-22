import { useCallback, useEffect, useReducer, useRef, useState, type KeyboardEvent } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { getRows } from "../api/commands";
import { STATUS_FILTERS, STATUS_LABELS } from "../status";
import { errorMessage, useSessionStore } from "../store/sessionStore";
import type { PlanSummary, Row } from "../types";

const PAGE_SIZE = 200;
const ROW_HEIGHT = 36;

function filterCount(key: string, summary: PlanSummary | null): number | null {
  if (!summary) return null;
  switch (key) {
    case "all":
      return summary.total;
    case "ok":
      return summary.ok;
    case "conflict":
      return summary.conflict;
    case "noTimestamp":
      return summary.noTimestamp;
    case "unchanged":
      return summary.unchanged;
    case "excluded":
      return summary.excluded;
    case "error":
      return summary.errors;
    case "done":
      return summary.done;
    default:
      return null;
  }
}

export function StatusFilterBar() {
  const filter = useSessionStore((state) => state.filter);
  const summary = useSessionStore((state) => state.summary);
  const setFilter = useSessionStore((state) => state.setFilter);
  const activeKey = STATUS_FILTERS.find((option) => JSON.stringify(option.kinds) === JSON.stringify(filter))?.key;

  return (
    <div className="filter-bar" role="tablist" aria-label="状態で絞り込み">
      {STATUS_FILTERS.map((option) => {
        const count = filterCount(option.key, summary);
        return (
          <button
            key={option.key}
            type="button"
            role="tab"
            aria-selected={activeKey === option.key}
            className={`filter-chip chip-${option.key}${activeKey === option.key ? " active" : ""}`}
            onClick={() => setFilter(option.kinds)}
          >
            {option.label}
            {count !== null && <span className="count">{count.toLocaleString()}</span>}
          </button>
        );
      })}
    </div>
  );
}

export function PreviewTable() {
  const rowsVersion = useSessionStore((state) => state.rowsVersion);
  const filter = useSessionStore((state) => state.filter);
  const selectedId = useSessionStore((state) => state.selectedId);
  const select = useSessionStore((state) => state.select);
  const setExcluded = useSessionStore((state) => state.setExcluded);
  const setManualName = useSessionStore((state) => state.setManualName);

  const [total, setTotal] = useState(0);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ id: number; value: string } | null>(null);
  const [, rerender] = useReducer((value: number) => value + 1, 0);
  const pages = useRef(new Map<number, Row[]>());
  const stalePages = useRef(new Map<number, Row[]>());
  const loading = useRef(new Set<number>());
  const generation = useRef(0);
  const scrollElement = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: total,
    getScrollElement: () => scrollElement.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 15,
  });
  const items = virtualizer.getVirtualItems();
  const firstPage = items.length > 0 ? Math.floor(items[0].index / PAGE_SIZE) : 0;
  const lastPage = items.length > 0 ? Math.floor(items[items.length - 1].index / PAGE_SIZE) : 0;

  const loadPage = useCallback(
    async (page: number) => {
      if (pages.current.has(page) || loading.current.has(page)) return;
      const current = generation.current;
      loading.current.add(page);
      try {
        const result = await getRows(page * PAGE_SIZE, PAGE_SIZE, filter);
        if (current !== generation.current) return;
        pages.current.set(page, result.rows);
        stalePages.current.delete(page);
        setTotal(result.total);
        setLoadError(null);
        const { selectedId: selected, selectedRow, select: choose } = useSessionStore.getState();
        const updated = result.rows.find((row) => row.id === selected);
        if (updated && updated !== selectedRow) choose(updated);
        rerender();
      } catch (reason) {
        if (current === generation.current) setLoadError(errorMessage(reason));
      } finally {
        if (current === generation.current) loading.current.delete(page);
      }
    },
    [filter],
  );

  // Keep showing the previous rows until the refreshed pages arrive to avoid flicker.
  useEffect(() => {
    generation.current += 1;
    stalePages.current = new Map([...stalePages.current, ...pages.current]);
    pages.current = new Map();
    loading.current = new Set();
    for (let page = firstPage; page <= lastPage; page += 1) void loadPage(page);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rowsVersion, loadPage]);

  useEffect(() => {
    for (let page = firstPage; page <= lastPage; page += 1) void loadPage(page);
  }, [firstPage, lastPage, loadPage]);

  const rowAt = (index: number): Row | undefined => {
    const page = Math.floor(index / PAGE_SIZE);
    const offset = index % PAGE_SIZE;
    return pages.current.get(page)?.[offset] ?? stalePages.current.get(page)?.[offset];
  };

  const commitEdit = () => {
    if (!editing) return;
    void setManualName(editing.id, editing.value);
    setEditing(null);
  };

  const onEditKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") commitEdit();
    if (event.key === "Escape") setEditing(null);
  };

  return (
    <div className="preview-table">
      <div className="table-header" role="row">
        <span>対象</span>
        <span>元の名前</span>
        <span />
        <span>新しい名前（ダブルクリックで編集）</span>
        <span>撮影日時</span>
        <span>状態</span>
      </div>
      <div className="table-body" ref={scrollElement}>
        {total === 0 && <p className="table-empty">{loadError ?? "表示する行がありません"}</p>}
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {items.map((item) => {
            const row = rowAt(item.index);
            const style = { transform: `translateY(${item.start}px)`, height: ROW_HEIGHT };
            if (!row) {
              return <div key={item.key} className="table-row placeholder" style={style} />;
            }
            const isEditing = editing?.id === row.id;
            return (
              <div
                key={item.key}
                role="row"
                className={`table-row${row.id === selectedId ? " selected" : ""}${row.excluded ? " excluded" : ""}`}
                style={style}
                onClick={() => select(row)}
              >
                <span>
                  <input
                    type="checkbox"
                    aria-label={`${row.originalName} を対象にする`}
                    checked={!row.excluded}
                    disabled={row.status.kind === "done"}
                    onClick={(event) => event.stopPropagation()}
                    onChange={(event) => void setExcluded(row.id, !event.target.checked)}
                  />
                </span>
                <span className="cell-name" title={`${row.directory}\n${row.originalName}`}>
                  {row.originalName}
                </span>
                <span className="cell-arrow" aria-hidden="true">
                  →
                </span>
                {isEditing ? (
                  <span className="cell-edit">
                    <input
                      autoFocus
                      value={editing.value}
                      onChange={(event) => setEditing({ id: row.id, value: event.target.value })}
                      onKeyDown={onEditKey}
                      onBlur={commitEdit}
                      spellCheck={false}
                    />
                  </span>
                ) : (
                  <span
                    className={`cell-name new-name${row.manual ? " manual" : ""}`}
                    title={row.status.detail ?? row.newName ?? ""}
                    onDoubleClick={() =>
                      row.status.kind !== "done" &&
                      setEditing({ id: row.id, value: row.newName ?? row.originalName })
                    }
                  >
                    {row.newName ?? "—"}
                  </span>
                )}
                <span className="cell-date">{row.takenAt ?? "—"}</span>
                <span>
                  <span className={`status-badge status-${row.status.kind}`} title={row.status.detail}>
                    {STATUS_LABELS[row.status.kind]}
                  </span>
                </span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
