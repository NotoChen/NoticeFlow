import { memo, useEffect, useMemo, useState } from "react";
import type { ApplicationInfo } from "../lib/tauri";
import { applicationIcon, applicationIconForBundle } from "../lib/tauri";

type AppPickerRow = {
  app: ApplicationInfo;
  bundleKey: string;
  searchText: string;
};

const missingIconCache = "__noticeflow_missing_icon__";
const iconSrcCache = new Map<string, string>();
const iconRequestCache = new Map<string, Promise<string | null>>();
const appPickerRowsCache = new WeakMap<ApplicationInfo[], AppPickerRow[]>();
const maxIconMemoryCacheEntries = 500;
const appPickerInitialLimit = 36;
const appPickerSearchLimit = 80;

function cachedIconSrc(key: string) {
  const cached = iconSrcCache.get(key);
  return cached && cached !== missingIconCache ? cached : "";
}

function rememberIconSrc(key: string, value: string | null) {
  if (iconSrcCache.has(key)) iconSrcCache.delete(key);
  iconSrcCache.set(key, value || missingIconCache);
  while (iconSrcCache.size > maxIconMemoryCacheEntries) {
    const oldestKey = iconSrcCache.keys().next();
    if (oldestKey.done) break;
    iconSrcCache.delete(oldestKey.value);
  }
}

function appPickerRows(apps: ApplicationInfo[]) {
  const cached = appPickerRowsCache.get(apps);
  if (cached) return cached;
  const rows = apps.map((app) => ({
    app,
    bundleKey: app.bundleId.toLowerCase(),
    searchText: `${app.name}\n${app.localizedName ?? ""}\n${app.bundleId}`.toLowerCase(),
  }));
  appPickerRowsCache.set(apps, rows);
  return rows;
}

export function AppPicker(props: {
  apps: ApplicationInfo[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  allowEmpty?: boolean;
  emptyLabel?: string;
}) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const appRows = useMemo(() => appPickerRows(props.apps), [props.apps]);
  const normalizedValue = props.value.trim();
  const valueKey = normalizedValue.toLowerCase();
  const selected = useMemo(
    () => appRows.find((row) => row.bundleKey === valueKey)?.app,
    [appRows, valueKey],
  );
  const text = search.trim().toLowerCase();
  const filteredApps = useMemo(() => {
    if (!open || props.disabled) return [];
    if (!text) {
      return appRows.slice(0, appPickerInitialLimit).map((row) => row.app);
    }
    return appRows
      .filter((row) => row.searchText.includes(text))
      .map((row) => row.app)
      .slice(0, appPickerSearchLimit);
  }, [appRows, open, props.disabled, text]);
  const fallbackValue = normalizedValue || (props.allowEmpty ? props.emptyLabel ?? "全部应用" : "");
  const displayValue = open && !props.disabled ? search : selected ? selected.localizedName || selected.name : fallbackValue;

  return (
    <div className="relative">
      <div className={`flex h-9 items-center gap-2 rounded-md border border-border bg-white px-2 ${props.disabled ? "opacity-70" : "focus-within:border-accent focus-within:shadow-[0_0_0_3px_rgba(22,163,74,0.12)]"}`}>
        <AppIcon app={selected} bundleId={props.value} size="sm" />
        <input
          className="h-full min-w-0 flex-1 border-0 bg-transparent text-sm outline-none"
          value={displayValue}
          disabled={props.disabled}
          onFocus={(event) => {
            if (props.disabled) return;
            setOpen(true);
            setSearch("");
            requestAnimationFrame(() => event.currentTarget.select());
          }}
          onChange={(event) => {
            setSearch(event.target.value);
            setOpen(true);
          }}
          onBlur={() => window.setTimeout(() => setOpen(false), 120)}
        />
      </div>
      {open && !props.disabled ? (
        <div className="absolute z-40 mt-1 max-h-72 w-full overflow-auto rounded-md border border-border bg-white py-1 shadow-lg">
          {props.allowEmpty ? (
            <button
              className="flex w-full items-center gap-2 px-2 py-2 text-left text-sm hover:bg-muted"
              onMouseDown={(event) => {
                event.preventDefault();
                props.onChange("");
                setSearch("");
                setOpen(false);
              }}
            >
              <AppIcon bundleId="" size="sm" />
              <span className="truncate">{props.emptyLabel ?? "全部应用"}</span>
            </button>
          ) : null}
          {filteredApps.map((app) => (
            <button
              key={app.bundleId}
              className="flex w-full items-center gap-2 px-2 py-2 text-left hover:bg-muted"
              onMouseDown={(event) => {
                event.preventDefault();
                props.onChange(app.bundleId);
                setSearch("");
                setOpen(false);
              }}
            >
              <AppIcon app={app} bundleId={app.bundleId} size="sm" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm">{app.localizedName || app.name}</span>
                <span className="block truncate text-[11px] text-subdued">
                  {app.localizedName && app.localizedName !== app.name ? `${app.name} · ${app.bundleId}` : app.bundleId}
                </span>
              </span>
            </button>
          ))}
          {!filteredApps.length ? <div className="px-3 py-4 text-center text-sm text-subdued">没有匹配的应用</div> : null}
        </div>
      ) : null}
    </div>
  );
}

export const AppIcon = memo(function AppIcon({ app, bundleId, size = "md" }: { app?: ApplicationInfo; bundleId?: string; size?: "sm" | "md" }) {
  const iconPath = app?.iconPath ?? "";
  const iconCacheKey = app?.iconCacheKey ?? "";
  const iconDataUrl = app?.iconDataUrl ?? "";
  const normalizedBundleId = bundleId?.trim() ?? "";
  const bundleKey = normalizedBundleId ? `bundle:${normalizedBundleId.toLowerCase()}` : "";
  const iconMemoryKey = iconPath ? `${iconPath}#${iconCacheKey || "unknown"}` : "";
  const initialSrc = iconDataUrl || (iconMemoryKey ? cachedIconSrc(iconMemoryKey) : "") || (bundleKey ? cachedIconSrc(bundleKey) : "") || "";
  const [src, setSrc] = useState(initialSrc);
  const label = app?.localizedName || app?.name || bundleId || "A";
  const dimension = size === "sm" ? "h-6 w-6" : "h-8 w-8";

  useEffect(() => {
    let cancelled = false;
    setSrc(initialSrc);
    const loadIcon = (key: string, loader: () => Promise<string | null>) => {
      const cached = iconSrcCache.get(key);
      if (cached) {
        if (cached === missingIconCache) {
          setSrc((current) => (current ? "" : current));
          return Promise.resolve(null);
        }
        setSrc((current) => (current !== cached ? cached : current));
        return Promise.resolve(cached);
      }
      let request = iconRequestCache.get(key);
      if (!request) {
        request = loader().then((dataUrl) => {
          rememberIconSrc(key, dataUrl);
          iconRequestCache.delete(key);
          return dataUrl;
        }).catch((error) => {
          rememberIconSrc(key, null);
          iconRequestCache.delete(key);
          throw error;
        });
        iconRequestCache.set(key, request);
      }
      request.then((dataUrl) => {
        if (!cancelled && dataUrl) setSrc(dataUrl);
      }).catch(() => {
        if (!cancelled) setSrc("");
      });
      return request;
    };
    const loadByBundle = () => {
      if (!normalizedBundleId) {
        setSrc("");
        return;
      }
      loadIcon(bundleKey, () => applicationIconForBundle(normalizedBundleId));
    };
    if (iconDataUrl) {
      setSrc(iconDataUrl);
      return;
    }
    if (!iconPath) {
      loadByBundle();
      return () => {
        cancelled = true;
      };
    }
    loadIcon(iconMemoryKey, () => applicationIcon(iconPath))
      .then((dataUrl) => {
        if (!dataUrl) {
          loadByBundle();
        }
      })
      .catch(() => {
        if (!cancelled) loadByBundle();
      });
    return () => {
      cancelled = true;
    };
  }, [bundleKey, iconCacheKey, iconDataUrl, iconMemoryKey, iconPath, initialSrc, normalizedBundleId]);

  if (src) {
    return <img className={`${dimension} shrink-0 rounded-md object-contain`} src={src} alt="" onError={() => setSrc("")} />;
  }

  return (
    <span className={`${dimension} grid shrink-0 place-items-center rounded-md bg-muted text-[11px] font-semibold text-subdued`}>
      {label.trim().slice(0, 1).toUpperCase() || "A"}
    </span>
  );
});
