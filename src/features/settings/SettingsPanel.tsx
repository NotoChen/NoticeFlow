import { useEffect, useMemo, useState } from "react";
import { Archive, ArrowLeft, Clock, Database, Download, FolderOpen, RefreshCw, RotateCcw, Save, ShieldCheck, X } from "lucide-react";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { AppIcon, AppPicker } from "../../components/AppPicker";
import { EmptyBlock, Field, StatusPill } from "../../components/FormBits";
import { Switch } from "../../components/Switch";
import { sameStringSetIgnoreOrder } from "../../lib/appModel";
import type {
  ActionQueueItem,
  ActionQueueStatus,
  ApplicationInfo,
  ArchiveStats,
  AutomationStatus,
  SettingsInfo,
  SystemDeleteAuditEntry,
} from "../../lib/tauri";
import { chooseDataDirectory, saveAppSettings } from "../../lib/tauri";

type OperationErrorScope = "general" | "update" | "permission" | "data" | "archive";
type OperationErrorState = {
  scope: OperationErrorScope;
  message: string;
} | null;

export function SettingsPanel(props: {
  info: SettingsInfo | null;
  status: AutomationStatus | null;
  apps: ApplicationInfo[];
  appCount: number;
  ruleCount: number;
  notificationCount: number;
  actionQueue: ActionQueueStatus | null;
  archiveStats: ArchiveStats | null;
  systemDeleteAudit: SystemDeleteAuditEntry[];
  refresh: () => Promise<void>;
  refreshMaintenance: () => Promise<void>;
  revealPath: (path: string) => Promise<void>;
  openFullDiskAccessSettings: () => Promise<void>;
  restoreHiddenNotifications: () => Promise<void>;
  compactArchive: () => Promise<void>;
  pruneArchive: (retentionDays: number) => Promise<void>;
  onSaved: (info: SettingsInfo) => void;
  onBack: () => void;
}) {
  const [operationError, setOperationError] = useState<OperationErrorState>(null);
  const [busy, setBusy] = useState("");
  const [updateMessage, setUpdateMessage] = useState("");
  const [retentionDays, setRetentionDays] = useState(90);
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [dataDirectory, setDataDirectory] = useState("");
  const [appFilterMode, setAppFilterMode] = useState<"exclude" | "include">("exclude");
  const [ignoredAppIdentifiers, setIgnoredAppIdentifiers] = useState<string[]>([]);

  const appById = useMemo(() => {
    const map = new Map<string, ApplicationInfo>();
    for (const app of props.apps) {
      map.set(app.bundleId.toLowerCase(), app);
    }
    return map;
  }, [props.apps]);

  useEffect(() => {
    if (!props.info) return;
    setLaunchAtLogin(props.info.launchAtLogin);
    setDataDirectory(props.info.dataDirectory);
    setAppFilterMode(props.info.appFilterMode ?? "exclude");
    setIgnoredAppIdentifiers(props.info.ignoredAppIdentifiers);
  }, [props.info]);

  const hasChanges = useMemo(() => {
    const originalIgnored = props.info?.ignoredAppIdentifiers ?? [];
    return (
      launchAtLogin !== (props.info?.launchAtLogin ?? false) ||
      dataDirectory.trim() !== (props.info?.dataDirectory ?? "") ||
      appFilterMode !== (props.info?.appFilterMode ?? "exclude") ||
      !sameStringSetIgnoreOrder(ignoredAppIdentifiers, originalIgnored)
    );
  }, [appFilterMode, dataDirectory, ignoredAppIdentifiers, launchAtLogin, props.info]);

  async function runOperation(name: string, operation: () => Promise<void>, scope: OperationErrorScope = "general") {
    setBusy(name);
    setOperationError(null);
    try {
      await operation();
    } catch (err) {
      if (scope === "update") setUpdateMessage("");
      setOperationError({
        scope,
        message: err instanceof Error ? err.message : String(err),
      });
    } finally {
      setBusy("");
    }
  }

  async function chooseDirectory() {
    await runOperation("choose-directory", async () => {
      const directory = await chooseDataDirectory();
      if (directory) setDataDirectory(cleanDirectoryPath(directory));
    }, "data");
  }

  async function saveSettings() {
    await runOperation("save-settings", async () => {
      const info = await saveAppSettings({
        launchAtLogin,
        dataDirectory: cleanDirectoryPath(dataDirectory),
        appFilterMode,
        ignoredAppIdentifiers,
      });
      props.onSaved(info);
    }, "general");
  }

  async function checkForUpdate() {
    await runOperation("check-update", async () => {
      setUpdateMessage("正在检查更新");
      const update = await check({ timeout: 15000 });
      if (!update) {
        setUpdateMessage("当前已是最新版本");
        return;
      }

      let downloaded = 0;
      let contentLength = 0;
      setUpdateMessage(`发现 ${update.version}，正在下载`);
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          downloaded = 0;
          contentLength = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setUpdateMessage(contentLength > 0 ? `正在下载 ${formatBytes(downloaded)} / ${formatBytes(contentLength)}` : `正在下载 ${formatBytes(downloaded)}`);
        } else if (event.event === "Finished") {
          setUpdateMessage("更新已下载，正在安装");
        }
      });
      setUpdateMessage("更新已安装，正在重启");
      await relaunch();
    }, "update");
  }

  function addIgnoredApp(bundleId: string) {
    const value = bundleId.trim();
    if (!value) return;
    setIgnoredAppIdentifiers((current) => {
      if (current.some((item) => item.toLowerCase() === value.toLowerCase())) return current;
      return [...current, value];
    });
  }

  function removeIgnoredApp(bundleId: string) {
    setIgnoredAppIdentifiers((current) => current.filter((item) => item.toLowerCase() !== bundleId.toLowerCase()));
  }

  const permissionChecked = !!props.info?.notificationDatabaseChecked;
  const permissionAccessible = permissionChecked && !!props.info?.notificationDatabaseAccessible;
  const permissionLabel = !permissionChecked ? "待检查" : permissionAccessible ? "可读取" : "待授权";
  const filterListLabel = appFilterMode === "include" ? "仅保留应用通知" : "忽略应用通知";
  const operationErrorFor = (scope: OperationErrorScope) =>
    operationError?.scope === scope ? <InlineOperationError message={operationError.message} /> : null;

  return (
    <div className="scrollbar h-full overflow-auto">
      <section className="mx-auto grid max-w-5xl gap-4">
        <div>
          <button className="inline-flex h-8 items-center gap-2 rounded-md border border-border bg-white px-3 text-sm hover:bg-muted" onClick={props.onBack}>
            <ArrowLeft size={15} />
            返回
          </button>
        </div>
        <div className="grid grid-cols-2 gap-3 min-[900px]:grid-cols-5">
          <Metric label="监听" value={props.info?.watcherRunning || props.status?.watcherRunning ? "运行中" : "未启动"} />
          <Metric label="权限" value={permissionLabel} />
          <Metric label="规则" value={String(props.ruleCount)} />
          <Metric label="通知" value={String(props.notificationCount)} />
          <Metric label="队列" value={props.actionQueue?.running ? `执行中 / ${props.actionQueue.pendingCount}` : String(props.actionQueue?.pendingCount ?? 0)} />
        </div>

        <section className="rounded-lg border border-border bg-white p-4 shadow-soft">
          <div className="mb-3 flex items-center justify-between gap-3">
            <h2 className="m-0 text-sm font-semibold">常规</h2>
            <button className="button-primary" disabled={!hasChanges || !!busy} onClick={saveSettings}>
              <Save size={14} />
              保存
            </button>
          </div>
          <div className="grid gap-3">
            <div className="flex items-center justify-between gap-4 rounded-md border border-border px-3 py-3">
              <div>
                <div className="text-sm font-medium">开机自启</div>
                <div className="mt-1 text-xs text-subdued">LaunchAgent</div>
              </div>
              <Switch checked={launchAtLogin} onCheckedChange={setLaunchAtLogin} />
            </div>
          </div>
          {operationErrorFor("general")}
        </section>

        <section className="rounded-lg border border-border bg-white p-4 shadow-soft">
          <div className="mb-3 flex items-center justify-between gap-3">
            <h2 className="m-0 text-sm font-semibold">软件更新</h2>
            <button className="button-secondary" disabled={!!busy} onClick={checkForUpdate}>
              {busy === "check-update" ? <RefreshCw size={14} className="animate-spin" /> : <Download size={14} />}
              检查更新
            </button>
          </div>
          {updateMessage ? <div className="rounded-md border border-border bg-muted px-3 py-2 text-sm text-slate-700">{updateMessage}</div> : null}
          {operationErrorFor("update")}
        </section>

        <section className="rounded-lg border border-border bg-white p-4 shadow-soft">
          <div className="mb-3 flex items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <ShieldCheck size={18} className={permissionAccessible ? "text-accent" : "text-amber-600"} />
              <h2 className="m-0 text-sm font-semibold">通知读取权限</h2>
            </div>
            <StatusPill ok={permissionAccessible} okText="可读取" badText={permissionChecked ? "需要授权" : "待检查"} />
          </div>
          {props.info?.notificationDatabaseError ? (
            <div className="mb-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800">
              {props.info.notificationDatabaseError}
            </div>
          ) : null}
          {operationErrorFor("permission")}
          <div className="flex flex-wrap gap-2">
            <button className="button-primary" onClick={() => runOperation("privacy", props.openFullDiskAccessSettings, "permission")} disabled={!!busy}>
              打开完全磁盘访问
            </button>
            <button className="button-secondary" onClick={() => runOperation("refresh", props.refresh, "permission")} disabled={!!busy}>
              <RefreshCw size={14} className={busy === "refresh" ? "animate-spin" : ""} />
              刷新状态
            </button>
            <button className="button-secondary" onClick={() => runOperation("restore-hidden", props.restoreHiddenNotifications, "permission")} disabled={!!busy}>
              <RotateCcw size={14} />
              恢复隐藏通知
            </button>
          </div>
        </section>

        <section className="rounded-lg border border-border bg-white p-4 shadow-soft">
          <div className="mb-3 flex items-center justify-between gap-3">
            <h2 className="m-0 text-sm font-semibold">数据位置</h2>
            <button className="button-secondary" disabled={!!busy} onClick={chooseDirectory}>
              <FolderOpen size={14} />
              选择目录
            </button>
          </div>
          <div className="grid gap-3">
            <Field label="数据目录">
              <input className="input font-mono text-xs" value={dataDirectory} onChange={(event) => setDataDirectory(event.target.value)} />
            </Field>
            <PathRow
              label="通知数据库"
              path={props.info?.notificationDatabasePath ?? ""}
              onReveal={(path) => runOperation("reveal-notifications", () => props.revealPath(path), "data")}
            />
            <PathRow
              label="本地归档"
              path={props.info?.notificationArchivePath ?? ""}
              onReveal={(path) => runOperation("reveal-archive", () => props.revealPath(path), "data")}
            />
            <PathRow
              label="规则文件"
              path={props.info?.rulesPath ?? ""}
              onReveal={(path) => runOperation("reveal-rules", () => props.revealPath(path), "data")}
            />
            <PathRow
              label="设置文件"
              path={props.info?.settingsPath ?? ""}
              onReveal={(path) => runOperation("reveal-settings", () => props.revealPath(path), "data")}
            />
            {operationErrorFor("data")}
          </div>
        </section>

        <section className="rounded-lg border border-border bg-white p-4 shadow-soft">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <Database size={18} className="text-subdued" />
              <h2 className="m-0 text-sm font-semibold">归档维护</h2>
            </div>
            <button className="button-secondary" onClick={() => runOperation("refresh-maintenance", props.refreshMaintenance, "archive")} disabled={!!busy}>
              <RefreshCw size={14} className={busy === "refresh-maintenance" ? "animate-spin" : ""} />
              刷新
            </button>
          </div>
          <div className="grid gap-3">
            <div className="grid grid-cols-2 gap-2 min-[900px]:grid-cols-4">
              <MiniMetric label="大小" value={formatBytes(props.archiveStats?.sizeBytes ?? 0)} />
              <MiniMetric label="通知" value={String(props.archiveStats?.notificationCount ?? 0)} />
              <MiniMetric label="隐藏/删除" value={`${props.archiveStats?.hiddenCount ?? 0}/${props.archiveStats?.systemDeletedCount ?? 0}`} />
              <MiniMetric label="历史/审计" value={`${props.archiveStats?.actionHistoryCount ?? 0}/${props.archiveStats?.systemDeleteAuditCount ?? 0}`} />
            </div>
            <div className="flex flex-wrap items-end gap-2">
              <Field label="保留天数">
                <input className="input w-28" type="number" min={1} max={3650} value={retentionDays} onChange={(event) => setRetentionDays(Math.max(1, Math.min(3650, Number(event.target.value) || 90)))} />
              </Field>
              <button className="button-secondary h-9" onClick={() => runOperation("prune-archive", () => props.pruneArchive(retentionDays), "archive")} disabled={!!busy}>
                <Archive size={14} />
                清理旧归档
              </button>
              <button className="button-secondary h-9" onClick={() => runOperation("compact-archive", props.compactArchive, "archive")} disabled={!!busy}>
                <Database size={14} />
                压缩数据库
              </button>
            </div>
            {operationErrorFor("archive")}
          </div>
        </section>

        <section className="rounded-lg border border-border bg-white p-4 shadow-soft">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
            <h2 className="m-0 text-sm font-semibold">应用过滤</h2>
            <div className="flex rounded-md border border-border bg-muted p-0.5">
              <button
                className={`h-8 rounded px-3 text-xs ${appFilterMode === "exclude" ? "bg-white shadow-soft" : "text-subdued"}`}
                onClick={() => setAppFilterMode("exclude")}
              >
                忽略所选
              </button>
              <button
                className={`h-8 rounded px-3 text-xs ${appFilterMode === "include" ? "bg-white shadow-soft" : "text-subdued"}`}
                onClick={() => setAppFilterMode("include")}
              >
                仅保留所选
              </button>
            </div>
          </div>
          <div className="grid gap-3">
            <AppPicker apps={props.apps} value="" onChange={addIgnoredApp} allowEmpty emptyLabel="选择应用" />
            <div className="grid gap-2">
              {ignoredAppIdentifiers.map((bundleId) => {
                const app = appById.get(bundleId.toLowerCase());
                return (
                  <div key={bundleId} className="flex items-center gap-3 rounded-md border border-border px-3 py-2">
                    <AppIcon app={app} bundleId={bundleId} size="sm" />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-medium">{app?.name || bundleId}</div>
                      <div className="truncate text-[11px] text-subdued">{bundleId}</div>
                    </div>
                    <button className="grid h-7 w-7 place-items-center rounded-md border border-border text-subdued hover:text-red-600" onClick={() => removeIgnoredApp(bundleId)} aria-label={`移除忽略应用 ${app?.name || bundleId}`} title="移除">
                      <X size={14} />
                    </button>
                  </div>
                );
              })}
              {!ignoredAppIdentifiers.length ? <EmptyBlock label={`未配置${filterListLabel}`} /> : null}
            </div>
          </div>
        </section>

        <section className="rounded-lg border border-border bg-white p-4 shadow-soft">
          <div className="mb-3 flex items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <Clock size={18} className="text-subdued" />
              <h2 className="m-0 text-sm font-semibold">动作队列</h2>
            </div>
            <div className="text-xs text-subdued">{props.actionQueue?.pendingCount ?? 0} / {props.actionQueue?.maxPendingCount ?? 0}</div>
          </div>
          <div className="grid gap-2">
            {props.actionQueue?.running ? <QueueRow item={props.actionQueue.running} label="执行中" strong /> : null}
            {props.actionQueue?.pending.slice(0, 8).map((item) => (
              <QueueRow key={item.id} item={item} label="等待" />
            ))}
            {!props.actionQueue?.running && !props.actionQueue?.pending.length ? <EmptyBlock label="暂无待执行动作" /> : null}
          </div>
        </section>

        <section className="rounded-lg border border-border bg-white p-4 shadow-soft">
          <div className="mb-3 flex items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <Archive size={18} className="text-subdued" />
              <h2 className="m-0 text-sm font-semibold">系统删除审计</h2>
            </div>
            <span className="text-xs text-subdued">{props.systemDeleteAudit.length} 条</span>
          </div>
          <div className="grid gap-2">
            {props.systemDeleteAudit.slice(0, 20).map((item) => (
              <div key={item.id} className="grid gap-1 rounded-md border border-border px-3 py-2">
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0 truncate text-sm font-medium">{item.title || "无标题"}</div>
                  <span className="shrink-0 rounded-full bg-slate-100 px-2 py-0.5 text-[11px] text-slate-600">
                    {item.systemRowsDeleted} 行
                  </span>
                </div>
                <div className="truncate text-xs text-subdued">{formatHistoryTime(item.timestamp)} · {item.appName || item.appIdentifier} · #{item.recordId}</div>
                {item.body ? <div className="line-clamp-2 text-xs text-slate-600">{item.body}</div> : null}
              </div>
            ))}
            {!props.systemDeleteAudit.length ? <EmptyBlock label="暂无系统删除记录" /> : null}
          </div>
        </section>
      </section>
    </div>
  );
}

function InlineOperationError({ message }: { message: string }) {
  return <div className="mt-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">{message}</div>;
}

function Metric({ label, value, compact }: { label: string; value: string; compact?: boolean }) {
  return (
    <div className={`rounded-lg border border-border bg-white shadow-soft ${compact ? "p-3" : "p-4"}`}>
      <div className="text-xs text-subdued">{label}</div>
      <div className={`${compact ? "text-base" : "text-xl"} mt-1 truncate font-semibold`}>{value}</div>
    </div>
  );
}

function MiniMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border bg-muted px-3 py-2">
      <div className="text-[11px] text-subdued">{label}</div>
      <div className="mt-1 truncate text-sm font-semibold">{value}</div>
    </div>
  );
}

function QueueRow({ item, label, strong }: { item: ActionQueueItem; label: string; strong?: boolean }) {
  return (
    <div className={`grid gap-1 rounded-md border px-3 py-2 ${strong ? "border-emerald-200 bg-emerald-50" : "border-border"}`}>
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0 truncate text-sm font-medium">{item.ruleName || "未命名规则"}</div>
        <span className="shrink-0 rounded-full bg-white px-2 py-0.5 text-[11px] text-slate-600">{label}</span>
      </div>
      <div className="truncate text-xs text-subdued">{formatHistoryTime(item.queuedAt)} · {item.actionCount} 个动作 · {item.appIdentifier}</div>
      <div className="truncate text-xs text-slate-600">{item.notificationTitle || "无标题"}</div>
    </div>
  );
}

function PathRow({ label, path, onReveal }: { label: string; path: string; onReveal: (path: string) => Promise<void> }) {
  return (
    <div className="grid grid-cols-[96px_minmax(0,1fr)_36px] items-center gap-2">
      <div className="text-xs text-subdued">{label}</div>
      <div className="truncate rounded-md border border-border bg-muted px-3 py-2 font-mono text-xs text-slate-700">{path || "-"}</div>
      <button className="grid h-9 w-9 place-items-center rounded-md border border-border bg-white" disabled={!path} onClick={() => path && onReveal(path)} aria-label={`在访达中显示${label}`} title="在访达中显示">
        <FolderOpen size={15} />
      </button>
    </div>
  );
}

function cleanDirectoryPath(path: string) {
  const trimmed = path.trim();
  if (trimmed === "/") return trimmed;
  return trimmed.replace(/\/+$/, "");
}

function formatHistoryTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / 1024 / 1024).toFixed(1)} MB`;
  return `${(value / 1024 / 1024 / 1024).toFixed(1)} GB`;
}
