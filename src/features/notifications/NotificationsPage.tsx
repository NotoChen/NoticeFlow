import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UIEvent } from "react";
import { ArrowLeft, Copy, RefreshCw, Search } from "lucide-react";
import { AppIcon, AppPicker } from "../../components/AppPicker";
import { EmptyBlock, ReadonlyField } from "../../components/FormBits";
import { previewVariables } from "../../lib/tauri";
import type { ApplicationInfo, NotificationRecord, VariablePreview } from "../../lib/tauri";

const formattedTimeCache = new Map<string, string>();
const maxFormattedTimeCacheEntries = 1000;

function formatNotificationTime(value: string) {
  const cached = formattedTimeCache.get(value);
  if (cached) return cached;
  const date = new Date(value);
  const formatted = Number.isNaN(date.getTime()) ? value : date.toLocaleString();
  formattedTimeCache.set(value, formatted);
  while (formattedTimeCache.size > maxFormattedTimeCacheEntries) {
    const oldestKey = formattedTimeCache.keys().next();
    if (oldestKey.done) break;
    formattedTimeCache.delete(oldestKey.value);
  }
  return formatted;
}

export function NotificationsPage(props: {
  apps: ApplicationInfo[];
  appById: Map<string, ApplicationInfo>;
  notifications: NotificationRecord[];
  loading: boolean;
  refresh: () => void;
  selectedRecordId: number | null;
  setSelectedRecordId: (id: number) => void;
  query: string;
  setQuery: (value: string) => void;
  appFilter: string;
  setAppFilter: (value: string) => void;
  openMenu: (record: NotificationRecord, x: number, y: number) => void;
  onBack: () => void;
}) {
  const selected = useMemo(
    () => props.notifications.find((item) => item.id === props.selectedRecordId) ?? props.notifications[0] ?? null,
    [props.notifications, props.selectedRecordId],
  );
  const [variablePreview, setVariablePreview] = useState<VariablePreview | null>(null);
  const [variableError, setVariableError] = useState("");
  useEffect(() => {
    if (!selected) {
      setVariablePreview(null);
      setVariableError("");
      return;
    }
    let cancelled = false;
    setVariablePreview(null);
    setVariableError("");
    previewVariables(selected.id, [])
      .then((preview) => {
        if (!cancelled) {
          setVariablePreview(preview);
          setVariableError("");
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setVariablePreview(null);
          setVariableError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selected]);
  const listResetKey = `${props.query}\n${props.appFilter}`;
  const emptyLabel = props.query || props.appFilter ? "没有符合筛选的通知" : "暂无通知记录";
  return (
    <section className="grid h-full min-h-0 grid-cols-[minmax(360px,.95fr)_minmax(320px,1.05fr)] gap-4">
      <div className="flex min-h-0 flex-col rounded-lg border border-border bg-white shadow-soft">
        <div className="grid grid-cols-[36px_minmax(0,1fr)_minmax(180px,240px)_36px] gap-2 border-b border-border p-3">
          <button
            className="grid h-9 w-9 place-items-center rounded-md border border-border bg-white"
            onClick={props.onBack}
            aria-label="返回规则面板"
            title="返回"
          >
            <ArrowLeft size={15} />
          </button>
          <label className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-2.5 text-subdued" size={15} />
            <input className="input input-search" value={props.query} onChange={(event) => props.setQuery(event.target.value)} />
          </label>
          <AppPicker apps={props.apps} value={props.appFilter} onChange={props.setAppFilter} allowEmpty emptyLabel="全部应用" />
          <button
            className="grid h-9 w-9 place-items-center rounded-md border border-border bg-white disabled:opacity-50"
            disabled={props.loading}
            onClick={props.refresh}
            aria-label="刷新通知记录"
            title="刷新"
          >
            <RefreshCw size={15} className={props.loading ? "animate-spin" : ""} />
          </button>
        </div>
        <NotificationList
          notifications={props.notifications}
          selectedRecordId={props.selectedRecordId}
          appById={props.appById}
          onSelect={props.setSelectedRecordId}
          openMenu={props.openMenu}
          resetKey={listResetKey}
          emptyLabel={emptyLabel}
        />
      </div>
      <div className="scrollbar min-h-0 overflow-auto rounded-lg border border-border bg-white p-4 shadow-soft">
        {selected ? (
          <NotificationDetail
            item={selected}
            app={props.appById.get(selected.appIdentifier.toLowerCase())}
            variablePreview={variablePreview}
            variableError={variableError}
          />
        ) : <EmptyBlock label="选择一条通知后查看详情" />}
      </div>
    </section>
  );
}

export function NotificationList(props: {
  notifications: NotificationRecord[];
  selectedRecordId: number | null;
  appById: Map<string, ApplicationInfo>;
  onSelect: (id: number) => void;
  openMenu?: (record: NotificationRecord, x: number, y: number) => void;
  resetKey?: string;
  emptyLabel?: string;
}) {
  const rowHeight = 88;
  const overscan = 8;
  const containerRef = useRef<HTMLDivElement | null>(null);
  const scrollAnimationFrameRef = useRef<number | null>(null);
  const pendingScrollTopRef = useRef(0);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(520);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const updateHeight = () => setHeight(element.clientHeight || 520);
    updateHeight();
    const observer = new ResizeObserver(updateHeight);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    return () => {
      if (scrollAnimationFrameRef.current !== null) {
        window.cancelAnimationFrame(scrollAnimationFrameRef.current);
      }
    };
  }, []);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const maxScrollTop = Math.max(0, props.notifications.length * rowHeight - height);
    if (scrollTop <= maxScrollTop) return;
    element.scrollTop = maxScrollTop;
    setScrollTop(maxScrollTop);
  }, [height, props.notifications.length, scrollTop]);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    element.scrollTop = 0;
    setScrollTop(0);
  }, [props.resetKey]);

  const startIndex = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const visibleCount = Math.ceil(height / rowHeight) + overscan * 2;
  const endIndex = Math.min(props.notifications.length, startIndex + visibleCount);
  const visibleItems = props.notifications.slice(startIndex, endIndex);
  const handleScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    pendingScrollTopRef.current = event.currentTarget.scrollTop;
    if (scrollAnimationFrameRef.current !== null) return;
    scrollAnimationFrameRef.current = window.requestAnimationFrame(() => {
      scrollAnimationFrameRef.current = null;
      setScrollTop(pendingScrollTopRef.current);
    });
  }, []);

  return (
    <div
      ref={containerRef}
      className="scrollbar min-h-0 flex-1 overflow-auto"
      onScroll={handleScroll}
    >
      {props.notifications.length ? (
        <>
          <div style={{ height: startIndex * rowHeight }} />
          {visibleItems.map((item) => {
            const app = props.appById.get(item.appIdentifier.toLowerCase());
            return (
              <button
                key={item.id}
                className={`block w-full border-b border-border px-4 py-3 text-left ${item.id === props.selectedRecordId ? "bg-emerald-50" : "hover:bg-muted"}`}
                style={{ minHeight: rowHeight }}
                onClick={() => props.onSelect(item.id)}
                onContextMenu={(event) => {
                  if (!props.openMenu) return;
                  event.preventDefault();
                  props.openMenu(item, event.clientX, event.clientY);
                }}
              >
                <MemoNotificationRow item={item} app={app} />
              </button>
            );
          })}
          <div style={{ height: Math.max(0, props.notifications.length - endIndex) * rowHeight }} />
        </>
      ) : (
        <div className="px-4 py-8 text-center text-sm text-subdued">{props.emptyLabel ?? "没有匹配的通知"}</div>
      )}
    </div>
  );
}

function NotificationRow({ item, app }: { item: NotificationRecord; app?: ApplicationInfo }) {
  return (
    <div className="flex items-start gap-3">
      <AppIcon app={app} bundleId={item.appIdentifier} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-3">
          <span className="truncate text-sm font-medium">{item.title || "(无标题)"}</span>
          <span className="shrink-0 text-xs text-subdued">{formatNotificationTime(item.deliveredAt)}</span>
        </div>
        <div className="mt-1 truncate text-xs text-subdued">{app?.localizedName || app?.name || item.appName || item.appIdentifier}</div>
        <div className="mt-1 line-clamp-2 text-sm text-slate-600">{item.body}</div>
      </div>
    </div>
  );
}

function NotificationDetail({
  item,
  app,
  variablePreview,
  variableError,
}: {
  item: NotificationRecord;
  app?: ApplicationInfo;
  variablePreview: VariablePreview | null;
  variableError: string;
}) {
  const urls = urlsFromPreview(variablePreview);
  const variableRows = variablePreview?.displayNames.map((name) => ({
    name,
    value: variablePreview.variables[name] ?? "",
  })) ?? [];
  return (
    <div className="grid gap-4">
      <div className="flex items-start gap-3">
        <AppIcon app={app} bundleId={item.appIdentifier} />
        <div className="min-w-0">
          <div className="text-base font-semibold">{item.title || "(无标题)"}</div>
          <div className="mt-1 text-xs text-subdued">{app?.localizedName || app?.name || item.appName || item.appIdentifier}</div>
        </div>
      </div>
      <ReadonlyField label="时间" value={formatNotificationTime(item.deliveredAt)} />
      <ReadonlyField label="副标题" value={item.subtitle} />
      <ReadonlyField label="内容" value={item.body} multiline />
      <div className="grid gap-2">
        <div className="text-xs text-subdued">链接</div>
        {urls.map((url, index) => (
          <div key={`${url}-${index}`} className="grid grid-cols-[minmax(0,1fr)_32px] items-center gap-2 rounded-md border border-border px-3 py-2">
            <div className="truncate font-mono text-xs text-slate-700">{url}</div>
            <button className="grid h-7 w-7 place-items-center rounded-md border border-border" onClick={() => navigator.clipboard?.writeText(url)} aria-label={`复制链接 ${index + 1}`} title="复制">
              <Copy size={13} />
            </button>
          </div>
        ))}
        {!urls.length ? <EmptyBlock label="未提取到链接" /> : null}
      </div>
      <div className="grid gap-2">
        <div className="text-xs text-subdued">变量</div>
        {variableError ? <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">{variableError}</div> : null}
        {variableRows.map((row) => (
          <div key={row.name} className="grid grid-cols-[112px_minmax(0,1fr)_32px] items-start gap-2 rounded-md border border-border px-3 py-2">
            <div className="truncate font-mono text-xs text-subdued">{row.name}</div>
            <div className="scrollbar max-h-24 min-w-0 overflow-auto whitespace-pre-wrap break-words text-xs text-slate-700">{row.value.trim() ? row.value : "空"}</div>
            <button className="grid h-7 w-7 place-items-center rounded-md border border-border" onClick={() => navigator.clipboard?.writeText(row.value)} aria-label={`复制变量 ${row.name}`} title="复制">
              <Copy size={13} />
            </button>
          </div>
        ))}
        {!variableRows.length && !variableError ? <EmptyBlock label="变量加载中" /> : null}
      </div>
    </div>
  );
}

const MemoNotificationRow = memo(NotificationRow);

function urlsFromPreview(preview: VariablePreview | null) {
  if (!preview) return [];
  const json = preview.variables.urls_json;
  if (json) {
    try {
      const parsed = JSON.parse(json);
      if (Array.isArray(parsed)) return parsed.filter((item): item is string => typeof item === "string");
    } catch {
      return [];
    }
  }
  return preview.variables.url ? [preview.variables.url] : [];
}
