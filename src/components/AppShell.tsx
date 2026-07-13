import { useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

export function ConfirmDialog(props: {
  title: string;
  message: string;
  cancelLabel: string;
  confirmLabel: string;
  destructive?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/20 p-6" onClick={props.onCancel}>
      <div className="w-full max-w-sm rounded-lg border border-border bg-white p-4 shadow-lg" onClick={(event) => event.stopPropagation()}>
        <h2 className="m-0 text-base font-semibold">{props.title}</h2>
        <p className="m-0 mt-2 text-sm text-subdued">{props.message}</p>
        <div className="mt-4 flex justify-end gap-2">
          <button className="button-secondary" onClick={props.onCancel}>{props.cancelLabel}</button>
          <button className={props.destructive ? "button-danger" : "button-primary"} onClick={props.onConfirm}>{props.confirmLabel}</button>
        </div>
      </div>
    </div>
  );
}

export function ContextMenu({ x, y, children }: { x: number; y: number; children: ReactNode }) {
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 176, height: 132 });
  useLayoutEffect(() => {
    const rect = menuRef.current?.getBoundingClientRect();
    if (!rect) return;
    setSize({ width: rect.width, height: rect.height });
  }, [children]);
  const left = Math.max(8, Math.min(x, window.innerWidth - size.width - 8));
  const top = Math.max(8, Math.min(y, window.innerHeight - size.height - 8));

  return (
    <div
      ref={menuRef}
      className="fixed z-50 w-44 overflow-hidden rounded-md border border-border bg-white py-1 text-sm shadow-lg"
      style={{ left, top }}
      onClick={(event) => event.stopPropagation()}
    >
      {children}
    </div>
  );
}
