import { useRef } from "react";
import { DEFAULT_TEMPLATE, useSessionStore } from "../store/sessionStore";
import type { ConflictResolution, ExtCase } from "../types";

const TOKENS = [
  { token: "{date:YYYYMMDD_HHmmss}", label: "撮影日時" },
  { token: "{date:YYYY-MM-DD}", label: "日付" },
  { token: "{name}", label: "元の名前" },
  { token: "{seq:3}", label: "連番" },
  { token: "{camera}", label: "機種" },
];

export function TemplateEditor() {
  const template = useSessionStore((state) => state.template);
  const templateError = useSessionStore((state) => state.templateError);
  const options = useSessionStore((state) => state.options);
  const recursive = useSessionStore((state) => state.recursive);
  const setTemplate = useSessionStore((state) => state.setTemplate);
  const setOptions = useSessionStore((state) => state.setOptions);
  const setRecursive = useSessionStore((state) => state.setRecursive);
  const input = useRef<HTMLInputElement>(null);

  const insert = (token: string) => {
    const element = input.current;
    const start = element?.selectionStart ?? template.length;
    const end = element?.selectionEnd ?? template.length;
    const next = template.slice(0, start) + token + template.slice(end);
    setTemplate(next);
    requestAnimationFrame(() => {
      element?.focus();
      element?.setSelectionRange(start + token.length, start + token.length);
    });
  };

  return (
    <section className="template-editor" aria-label="命名テンプレート">
      <div className="template-row">
        <label className="field-label" htmlFor="template-input">
          命名テンプレート
        </label>
        <input
          id="template-input"
          ref={input}
          className={templateError ? "template-input invalid" : "template-input"}
          value={template}
          spellCheck={false}
          onChange={(event) => setTemplate(event.target.value)}
        />
        <button type="button" className="ghost" onClick={() => setTemplate(DEFAULT_TEMPLATE)}>
          初期値に戻す
        </button>
      </div>
      <div className="token-row">
        {TOKENS.map(({ token, label }) => (
          <button key={token} type="button" className="token" onClick={() => insert(token)} title={token}>
            {label}
            <code>{token}</code>
          </button>
        ))}
        <span className="hint">拡張子は元のまま付きます。上記以外の文字は固定文字として入ります。</span>
      </div>
      {templateError && (
        <p className="field-error" role="alert">
          {templateError}
        </p>
      )}
      <div className="option-row">
        <label>
          衝突したとき
          <select
            value={options.onConflict}
            onChange={(event) => setOptions({ onConflict: event.target.value as ConflictResolution })}
          >
            <option value="suffix">連番（_001）を付ける</option>
            <option value="skip">スキップする</option>
          </select>
        </label>
        <label>
          拡張子
          <select
            value={options.extCase}
            onChange={(event) => setOptions({ extCase: event.target.value as ExtCase })}
          >
            <option value="keep">そのまま</option>
            <option value="lower">小文字にする</option>
            <option value="upper">大文字にする</option>
          </select>
        </label>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={options.skipAlreadyNamed}
            onChange={(event) => setOptions({ skipAlreadyNamed: event.target.checked })}
          />
          変更済みの名前はそのままにする
        </label>
        <label className="checkbox">
          <input type="checkbox" checked={recursive} onChange={(event) => setRecursive(event.target.checked)} />
          フォルダはサブフォルダも読み込む
        </label>
      </div>
    </section>
  );
}
