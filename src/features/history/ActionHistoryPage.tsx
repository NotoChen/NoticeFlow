import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, ChevronDown, ChevronUp, History, RefreshCw, Trash2 } from "lucide-react";
import { EmptyBlock } from "../../components/FormBits";
import {
  actionTypeLabel,
  filterActionHistory,
  formatHistoryTime,
  originLabel,
  parseVariableSnapshot,
  shortId,
} from "../../lib/historyModel";
import type { HistoryOriginFilter, HistoryStatusFilter } from "../../lib/historyModel";
import type { ActionHistoryEntry, AutomationRule } from "../../lib/tauri";

const PAGE_SIZE = 60;

export function ActionHistoryPage(props: {
  items: ActionHistoryEntry[];
  rules: AutomationRule[];
  initialRuleId?: string;
  loading: boolean;
  refresh: () => void;
  clearHistory: () => Promise<void>;
  onBack: () => void;
}) {
  const [ruleFilter, setRuleFilter] = useState(props.initialRuleId ?? "");
  const [statusFilter, setStatusFilter] = useState<HistoryStatusFilter>("all");
  const [originFilter, setOriginFilter] = useState<HistoryOriginFilter>("all");
  const [expandedId, setExpandedId] = useState("");
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const [clearError, setClearError] = useState("");
  const [clearing, setClearing] = useState(false);

  useEffect(() => {
    setRuleFilter(props.initialRuleId ?? "");
    setStatusFilter("all");
    setOriginFilter("all");
    setExpandedId("");
    setVisibleCount(PAGE_SIZE);
    setClearError("");
  }, [props.initialRuleId]);

  const filtered = useMemo(
    () => filterActionHistory(props.items, ruleFilter, statusFilter, originFilter),
    [props.items, ruleFilter, statusFilter, originFilter],
  );
  const visible = filtered.slice(0, visibleCount);

  const ruleOptions = useMemo(() => {
    const seen = new Map<string, string>();
    for (const rule of props.rules) seen.set(rule.id, rule.name || "未命名规则");
    for (const item of props.items) {
      if (!seen.has(item.ruleId)) seen.set(item.ruleId, item.ruleName || "未命名规则");
    }
    return [...seen.entries()];
  }, [props.rules, props.items]);

  const runClear = async () => {
    setClearing(true);
    setClearError("");
    try {
      await props.clearHistory();
    } catch (err) {
      setClearError(err instanceof Error ? err.message : String(err));
    } finally {
      setClearing(false);
    }
  };

  return (
    <div className="scrollbar h-full min-h-0 overflow-auto">
      <section className="mx-auto grid w-full max-w-4xl content-start gap-4">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <button className="button-secondary h-9" onClick={props.onBack}>
              <ArrowLeft size={15} />
              返回
            </button>
            <History size={18} className="text-subdued" />
            <h1 className="m-0 text-base font-semibold">执行历史</h1>
            <span className="text-xs text-subdued">
              {filtered.length === props.items.length
                ? `${props.items.length} 条`
                : `${filtered.length} / ${props.items.length} 条`}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <button className="button-secondary h-9" onClick={props.refresh} disabled={props.loading}>
              <RefreshCw size={14} className={props.loading ? "animate-spin" : ""} />
              刷新
            </button>
            <button className="button-secondary h-9" onClick={runClear} disabled={clearing || !props.items.length}>
              <Trash2 size={14} />
              清空全部
            </button>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-white p-3 shadow-soft">
          <select className="input h-9 w-56" value={ruleFilter} onChange={(event) => { setRuleFilter(event.target.value); setVisibleCount(PAGE_SIZE); }}>
            <option value="">全部规则</option>
            {ruleOptions.map(([id, name]) => (
              <option key={id} value={id}>{name}</option>
            ))}
          </select>
          <select className="input h-9 w-32" value={statusFilter} onChange={(event) => { setStatusFilter(event.target.value as HistoryStatusFilter); setVisibleCount(PAGE_SIZE); }}>
            <option value="all">全部状态</option>
            <option value="success">仅成功</option>
            <option value="failure">仅失败</option>
          </select>
          <select className="input h-9 w-32" value={originFilter} onChange={(event) => { setOriginFilter(event.target.value as HistoryOriginFilter); setVisibleCount(PAGE_SIZE); }}>
            <option value="all">全部来源</option>
            <option value="auto">自动执行</option>
            <option value="test">手动测试</option>
          </select>
        </div>

        {clearError ? <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">{clearError}</div> : null}

        <div className="grid gap-2">
          {visible.map((item) => (
            <HistoryEntryCard
              key={item.id}
              item={item}
              expanded={expandedId === item.id}
              toggle={() => setExpandedId(expandedId === item.id ? "" : item.id)}
            />
          ))}
          {!filtered.length ? <EmptyBlock label={props.items.length ? "没有符合筛选条件的记录" : "暂无执行历史"} /> : null}
          {filtered.length > visibleCount ? (
            <button className="button-secondary mx-auto h-9" onClick={() => setVisibleCount(visibleCount + PAGE_SIZE)}>
              加载更多（剩余 {filtered.length - visibleCount} 条）
            </button>
          ) : null}
        </div>
      </section>
    </div>
  );
}

function HistoryEntryCard(props: { item: ActionHistoryEntry; expanded: boolean; toggle: () => void }) {
  const { item, expanded } = props;
  const variables = parseVariableSnapshot(item.variablesJson);
  const hasDetails = Boolean(item.output) || variables.length > 0 || item.message.length > 160;
  return (
    <div className="grid gap-1 rounded-md border border-border bg-white px-3 py-2 shadow-soft">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <span className="min-w-0 truncate text-sm font-medium">{item.ruleName || "未命名规则"}</span>
          <span className={`shrink-0 rounded-full px-2 py-0.5 text-[11px] ${(item.origin ?? "auto") === "test" ? "bg-sky-50 text-sky-700" : "bg-slate-100 text-slate-600"}`}>
            {originLabel(item.origin ?? "auto")}
          </span>
        </div>
        <span className={`shrink-0 rounded-full px-2 py-0.5 text-[11px] ${item.success ? "bg-emerald-50 text-accent" : "bg-red-50 text-red-600"}`}>
          {item.success ? "成功" : "失败"}
        </span>
      </div>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-subdued">
        <span>{formatHistoryTime(item.timestamp)}</span>
        <span>{actionTypeLabel(item.actionType)} #{(item.actionIndex ?? 0) + 1}</span>
        <span>{item.durationMs}ms</span>
        <span>尝试 {item.attemptCount ?? 1}</span>
        {item.queueId ? <span className="font-mono">队列 {shortId(item.queueId)}</span> : null}
        <span className="min-w-0 truncate">{item.notificationTitle || "无标题"}</span>
      </div>
      <div className={`${expanded ? "" : "line-clamp-2"} text-xs text-slate-600`}>{item.message}</div>
      {expanded && item.output ? (
        <pre className="scrollbar mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-muted p-2 text-[11px] text-slate-700">{item.output}</pre>
      ) : null}
      {expanded && variables.length ? (
        <div className="mt-1 grid max-h-56 gap-1 overflow-auto rounded-md border border-border bg-muted p-2">
          {variables.map(([key, value]) => (
            <div key={key} className="grid grid-cols-[120px_minmax(0,1fr)] gap-2 text-[11px]">
              <div className="truncate font-mono text-subdued">{key}</div>
              <div className="whitespace-pre-wrap break-words text-slate-700">{value || "空"}</div>
            </div>
          ))}
        </div>
      ) : null}
      {hasDetails ? (
        <button className="mt-1 inline-flex w-fit items-center gap-1 rounded border border-border px-2 py-1 text-[11px] text-subdued hover:bg-muted" onClick={props.toggle}>
          {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
          {expanded ? "收起" : item.output ? "输出与详情" : "详情"}
        </button>
      ) : null}
    </div>
  );
}
