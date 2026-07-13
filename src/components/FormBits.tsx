import type { ReactNode } from "react";
import { Plus, Trash2 } from "lucide-react";

export function Header({ title, onAdd }: { title: string; onAdd: () => void }) {
  return (
    <div className="mb-3 flex items-center justify-between">
      <h2 className="m-0 text-sm font-semibold">{title}</h2>
      <button className="inline-flex h-8 items-center gap-2 rounded-md border border-border bg-white px-3 text-sm" onClick={onAdd}>
        <Plus size={14} />
        添加
      </button>
    </div>
  );
}

export function TabLabel({ label, count, warn }: { label: string; count?: number; warn?: boolean }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span>{label}</span>
      {typeof count === "number" ? (
        <span className={`rounded-full px-1.5 py-0.5 text-[11px] leading-none ${warn ? "bg-amber-100 text-amber-700" : "bg-slate-100 text-subdued"}`}>
          {count}
        </span>
      ) : null}
      {warn && typeof count !== "number" ? <span className="h-1.5 w-1.5 rounded-full bg-amber-500" /> : null}
    </span>
  );
}

export function EmptyBlock({ label, onClick }: { label: string; onClick?: () => void }) {
  const className = "rounded-md border border-dashed border-border bg-muted/50 px-3 py-4 text-center text-sm text-subdued";
  if (!onClick) {
    return <div className={className}>{label}</div>;
  }
  return (
    <button className={`${className} hover:border-accent hover:bg-emerald-50 hover:text-accent`} onClick={onClick}>
      <Plus size={14} className="mr-1 inline" />
      {label}
    </button>
  );
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label className="grid gap-1 text-sm"><span className="text-xs text-subdued">{label}</span>{children}</label>;
}

export function ReadonlyField({ label, value, multiline }: { label: string; value: string; multiline?: boolean }) {
  const displayValue = value.trim() ? value : "空";
  return (
    <div className="grid gap-1 text-sm">
      <span className="text-xs text-subdued">{label}</span>
      <div className={`scrollbar rounded-md border border-border bg-muted px-3 py-2 text-sm text-slate-700 ${multiline ? "max-h-28 min-h-9 overflow-auto whitespace-pre-wrap break-words" : "h-9 truncate"}`}>
        {displayValue}
      </div>
    </div>
  );
}

export function TextParam({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return <Field label={label}><input className="input" value={value} onChange={(event) => onChange(event.target.value)} /></Field>;
}

export function TextAreaParam({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return <Field label={label}><textarea className="input min-h-28 resize-y py-2" value={value} onChange={(event) => onChange(event.target.value)} /></Field>;
}

export function RemoveButton({ onClick }: { onClick: () => void }) {
  return (
    <button className="grid h-8 w-8 place-items-center rounded-md border border-border text-red-600" onClick={onClick} aria-label="删除此项" title="删除">
      <Trash2 size={14} />
    </button>
  );
}

export function StatusPill({ ok, okText, badText }: { ok: boolean; okText: string; badText: string }) {
  return (
    <span className={`rounded-full px-2 py-1 text-xs ${ok ? "bg-emerald-50 text-accent" : "bg-amber-50 text-amber-700"}`}>
      {ok ? okText : badText}
    </span>
  );
}
