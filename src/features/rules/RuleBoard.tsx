import type { PointerEvent } from "react";
import { GripVertical, Plus, RefreshCw, Search, Settings } from "lucide-react";
import { AppIcon, AppPicker } from "../../components/AppPicker";
import { Switch } from "../../components/Switch";
import type { ApplicationInfo, AutomationRule, NotificationRecord } from "../../lib/tauri";
import { NotificationList } from "../notifications/NotificationsPage";

export function RuleBoard(props: {
  apps: ApplicationInfo[];
  rules: AutomationRule[];
  appById: Map<string, ApplicationInfo>;
  notifications: NotificationRecord[];
  notificationSelectedRecordId: number | null;
  setNotificationSelectedRecordId: (id: number) => void;
  notificationQuery: string;
  setNotificationQuery: (value: string) => void;
  notificationAppFilter: string;
  setNotificationAppFilter: (value: string) => void;
  draggingId: string;
  loading: boolean;
  createRule: () => void;
  refreshAll: () => void;
  openSettings: () => void;
  selectRule: (rule: AutomationRule) => void;
  toggleRule: (rule: AutomationRule, enabled: boolean) => void;
  startDrag: (event: PointerEvent, rule: AutomationRule) => void;
  moveDrag: (event: PointerEvent) => void;
  endDrag: () => void;
  openContextMenu: (rule: AutomationRule, x: number, y: number) => void;
  openNotificationMenu: (record: NotificationRecord, x: number, y: number) => void;
}) {
  const notificationListResetKey = `${props.notificationQuery}\n${props.notificationAppFilter}`;
  const notificationEmptyLabel = props.notificationQuery || props.notificationAppFilter ? "没有符合筛选的通知" : "暂无通知记录";

  return (
    <section className="flex h-full min-h-0 flex-col rounded-lg border border-border bg-white shadow-soft">
      <div className="grid shrink-0 grid-cols-[minmax(320px,.78fr)_minmax(520px,1.35fr)] gap-4 border-b border-border p-3">
        <div className="grid grid-cols-[minmax(0,1fr)_minmax(160px,220px)] gap-2">
          <label className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-2.5 text-subdued" size={15} />
            <input className="input input-search" value={props.notificationQuery} onChange={(event) => props.setNotificationQuery(event.target.value)} />
          </label>
          <AppPicker apps={props.apps} value={props.notificationAppFilter} onChange={props.setNotificationAppFilter} allowEmpty emptyLabel="全部应用" />
        </div>
        <div className="flex items-center justify-end gap-2">
          <button className="button-primary h-9" onClick={props.createRule}>
            <Plus size={15} />
            新建规则
          </button>
          <button className="grid h-9 w-9 place-items-center rounded-md border border-border bg-white disabled:opacity-50" disabled={props.loading} onClick={props.refreshAll} aria-label="刷新全部状态" title="刷新全部状态">
            <RefreshCw size={15} className={props.loading ? "animate-spin" : ""} />
          </button>
          <button className="grid h-9 w-9 place-items-center rounded-md border border-border bg-white" onClick={props.openSettings} aria-label="设置" title="设置">
            <Settings size={15} />
          </button>
        </div>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(320px,.78fr)_minmax(520px,1.35fr)]">
        <section className="flex min-h-0 flex-col border-r border-border">
          <NotificationList
            notifications={props.notifications}
            selectedRecordId={props.notificationSelectedRecordId}
            appById={props.appById}
            onSelect={props.setNotificationSelectedRecordId}
            openMenu={props.openNotificationMenu}
            resetKey={notificationListResetKey}
            emptyLabel={notificationEmptyLabel}
          />
        </section>

        <section className="scrollbar min-h-0 overflow-auto p-3">
          <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-4">
            {props.rules.length ? props.rules.map((rule) => {
              const appId = rule.appIdentifiers?.[0] ?? "";
              const app = props.appById.get(appId.toLowerCase());
              const isDragging = props.draggingId === rule.id;
              return (
                <div
                  key={rule.id}
                  data-rule-card-id={rule.id}
                  role="button"
                  tabIndex={0}
                  aria-label={`编辑规则 ${rule.name || "未命名规则"}`}
                  className={`cursor-grab text-left transition-all active:cursor-grabbing ${isDragging ? "scale-95 opacity-20" : "opacity-100"}`}
                  onClick={() => props.selectRule(rule)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") props.selectRule(rule);
                  }}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    props.openContextMenu(rule, event.clientX, event.clientY);
                  }}
                  onPointerDown={(event) => props.startDrag(event, rule)}
                  onPointerMove={props.moveDrag}
                  onPointerUp={props.endDrag}
                  onPointerCancel={props.endDrag}
                >
                  <RuleCard
                    rule={rule}
                    app={app}
                    onToggle={(enabled) => props.toggleRule(rule, enabled)}
                  />
                </div>
              );
            }) : (
              <div className="rounded-lg border border-dashed border-border bg-white px-4 py-10 text-center text-sm text-subdued">
                暂无规则
              </div>
            )}
          </div>
        </section>
      </div>
    </section>
  );
}

export function RuleCard({
  rule,
  app,
  dragging,
  onToggle,
}: {
  rule: AutomationRule;
  app?: ApplicationInfo;
  dragging?: boolean;
  onToggle?: (enabled: boolean) => void;
}) {
  const appId = rule.appIdentifiers?.[0] ?? "";
  const enabled = rule.enabled ?? true;
  return (
    <div className={`min-h-[156px] rounded-lg border bg-white p-4 shadow-soft ${enabled ? "border-border" : "border-slate-200 opacity-65"} ${dragging ? "shadow-lg" : ""}`}>
      <div className="mb-4 flex items-start gap-3">
        <AppIcon app={app} bundleId={appId} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-xs text-subdued">{app?.name || appId || "未选择应用"}</div>
          <div className="mt-1 line-clamp-2 text-base font-semibold">{rule.name || "未命名规则"}</div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {onToggle ? (
            <span
              className="inline-flex"
              onClick={(event) => event.stopPropagation()}
              onPointerDown={(event) => event.stopPropagation()}
              onKeyDown={(event) => event.stopPropagation()}
            >
              <Switch checked={enabled} onCheckedChange={onToggle} />
            </span>
          ) : null}
          <GripVertical size={16} className="text-subdued" />
        </div>
      </div>
      <div className="grid grid-cols-4 gap-2 text-center">
        <CardMetric label="匹配" value={rule.matchConditions?.length ?? 0} />
        <CardMetric label="变量" value={rule.variableExtractions?.length ?? 0} />
        <CardMetric label="动作" value={rule.actions?.length ?? 0} />
        <CardMetric label="命中" value={rule.hitCount ?? 0} />
      </div>
    </div>
  );
}

function CardMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-md bg-muted px-2 py-2">
      <div className="text-sm font-semibold">{value}</div>
      <div className="mt-0.5 text-[11px] text-subdued">{label}</div>
    </div>
  );
}
