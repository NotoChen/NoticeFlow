import { useMemo, useState } from "react";
import * as Tabs from "@radix-ui/react-tabs";
import { ArrowLeft, Copy, FileCode2, Play, Plus, RefreshCw, Save, Search, SearchCheck, Trash2, Workflow } from "lucide-react";
import { AppPicker } from "../../components/AppPicker";
import { EmptyBlock, Field, Header, ReadonlyField, RemoveButton, StatusPill, TabLabel, TextAreaParam, TextParam } from "../../components/FormBits";
import { Switch } from "../../components/Switch";
import { NotificationList } from "../notifications/NotificationsPage";
import { chooseScriptFile } from "../../lib/tauri";
import type {
  ActionConfig,
  ApplicationInfo,
  AutomationRule,
  MatchCondition,
  NotificationRecord,
  VariableExtractionRule,
  VariablePreview,
} from "../../lib/tauri";
import type { RuleEditorTab, RuleIssue } from "../../lib/appModel";

const operators = [
  ["equals", "等于"],
  ["not_equals", "不等于"],
  ["contains", "包含"],
  ["not_contains", "不包含"],
  ["starts_with", "开头是"],
  ["ends_with", "结尾是"],
  ["regex", "正则匹配"],
  ["not_regex", "正则不匹配"],
  ["is_empty", "为空"],
  ["is_not_empty", "不为空"],
] as const;

const actionTypes = [
  ["open_url", "打开链接"],
  ["open_app", "打开应用"],
  ["activate_app", "激活应用"],
  ["send_notification", "发送通知"],
  ["run_shell", "Shell"],
  ["run_applescript", "AppleScript"],
  ["run_javascript", "JavaScript"],
  ["run_python", "Python"],
  ["http_request", "HTTP 请求"],
] as const;

const actionGroups = [
  {
    id: "open",
    label: "打开",
    types: ["open_url", "open_app", "activate_app"],
  },
  {
    id: "script",
    label: "脚本",
    types: ["run_shell", "run_applescript", "run_javascript", "run_python"],
  },
  {
    id: "message",
    label: "通知与网络",
    types: ["send_notification", "http_request"],
  },
] as const;

const actionTemplates: Array<{ label: string; action: ActionConfig }> = [
  { label: "打开首个 URL", action: { type: "open_url", parameters: { url: "{{url}}" } } },
  {
    label: "打开全部 URL",
    action: {
      type: "run_shell",
      parameters: {
        shell: "bash",
        shell_mode: "standard",
        timeout_seconds: "60",
        script: "while IFS= read -r _noticeflow_item; do\n  [ -n \"$_noticeflow_item\" ] && open \"$_noticeflow_item\"\ndone <<'EOF'\n{{urls_join:\\n}}\nEOF",
      },
    },
  },
  {
    label: "发送通知",
    action: { type: "send_notification", parameters: { title: "{{title}}", body: "{{body}}" } },
  },
  {
    label: "Webhook",
    action: {
      type: "http_request",
      parameters: {
        method: "POST",
        url: "",
        headers: "{\"Content-Type\":\"application/json\"}",
        body: "{\"app\":{{json:app_name}},\"title\":{{json:title}},\"url\":{{json:url}}}",
        retry_count: "1",
        retry_interval_seconds: "2",
      },
    },
  },
  {
    label: "本地脚本",
    action: { type: "run_shell", parameters: { shell: "bash", shell_mode: "standard", script: "", timeout_seconds: "60" } },
  },
];

const tabContentClass = "min-h-0 flex-1 overflow-hidden data-[state=active]:flex data-[state=active]:flex-col data-[state=inactive]:hidden";

export function RuleEditor(props: {
  rule: AutomationRule | null;
  apps: ApplicationInfo[];
  variableNames: string[];
  preview: VariablePreview;
  selectedRecordId: number | null;
  issues: RuleIssue[];
  tab: RuleEditorTab;
  setTab: (value: RuleEditorTab) => void;
  updateEditing: (patch: Partial<AutomationRule>) => void;
  updateCondition: (index: number, patch: Partial<MatchCondition>) => void;
  updateVariable: (index: number, patch: Partial<VariableExtractionRule>) => void;
  updateAction: (index: number, patch: Partial<ActionConfig>) => void;
  updateActionParam: (index: number, key: string, value: string) => void;
  isDirty: boolean;
  canRunPreviewActions: boolean;
  onSave: () => void;
  onRunMatchOnly: () => void;
  onRunTest: () => void;
  onBack: () => void;
}) {
  const { rule } = props;
  if (!rule) {
    return (
      <section className="grid min-h-0 place-items-center rounded-lg border border-border bg-white">
        <div className="text-center text-sm text-subdued">
          <Workflow className="mx-auto mb-3" size={28} />
          选择一条规则或新建规则
        </div>
      </section>
    );
  }

  const addCondition = () =>
    props.updateEditing({
      matchConditions: [
        ...(rule.matchConditions ?? []),
        { variableName: "title", operatorType: "contains", expectedValue: "", caseSensitive: false },
      ],
    });
  const addVariable = () =>
    props.updateEditing({
      variableExtractions: [
        ...(rule.variableExtractions ?? []),
        { name: "", source: "body", method: "regex", pattern: "", groupIndex: 1 },
      ],
    });
  const addAction = (action: ActionConfig = { type: "open_url", parameters: defaultParameters("open_url") }) =>
    props.updateEditing({
      actions: [...(rule.actions ?? []), cloneActionConfig(action)],
    });
  const conditions = rule.matchConditions ?? [];
  const variables = rule.variableExtractions ?? [];
  const actions = rule.actions ?? [];
  const tabHasIssue = (tab: RuleEditorTab) => props.issues.some((issue) => issue.tab === tab);
  const currentValueFor = (name: string) => {
    if (props.selectedRecordId === null) return "未选择样本";
    return props.preview.variables[name] ?? "";
  };

  return (
    <section className="flex min-h-0 flex-col rounded-lg border border-border bg-white shadow-soft">
      <div className="flex shrink-0 cursor-default items-center justify-between gap-3 border-b border-border px-3 py-2">
        <button className="inline-flex h-8 items-center gap-2 rounded-md border border-border bg-white px-3 text-sm hover:bg-muted" onClick={props.onBack}>
          <ArrowLeft size={15} />
          返回规则列表
        </button>
        <div className="flex items-center gap-2">
          <StatusPill ok={!props.isDirty} okText="已保存" badText="未保存" />
          <button className="inline-flex h-8 items-center gap-2 rounded-md border border-border bg-white px-3 text-sm disabled:opacity-50" disabled={!props.canRunPreviewActions} onClick={props.onRunMatchOnly} title="只检查当前预览通知是否匹配，不执行动作">
            <SearchCheck size={15} />
            仅匹配
          </button>
          <button className="inline-flex h-8 items-center gap-2 rounded-md border border-border bg-white px-3 text-sm disabled:opacity-50" disabled={!props.canRunPreviewActions} onClick={props.onRunTest} title="用当前预览通知测试规则">
            <Play size={15} />
            测试
          </button>
          <button className="inline-flex h-8 items-center gap-2 rounded-md bg-ink px-3 text-sm text-white" onClick={props.onSave} title="保存规则">
            <Save size={15} />
            保存
          </button>
        </div>
      </div>
      <Tabs.Root value={props.tab} onValueChange={(value) => props.setTab(value as RuleEditorTab)} className="flex min-h-0 flex-1 flex-col">
        <Tabs.List className="flex border-b border-border p-2 text-sm">
          <Tabs.Trigger value="basic" className="rounded px-3 py-1.5 data-[state=active]:bg-muted">
            <TabLabel label="基本" warn={tabHasIssue("basic")} />
          </Tabs.Trigger>
          <Tabs.Trigger value="match" className="rounded px-3 py-1.5 data-[state=active]:bg-muted">
            <TabLabel label="匹配逻辑" count={conditions.length} warn={tabHasIssue("match")} />
          </Tabs.Trigger>
          <Tabs.Trigger value="variables" className="rounded px-3 py-1.5 data-[state=active]:bg-muted">
            <TabLabel label="变量提取" count={variables.length} />
          </Tabs.Trigger>
          <Tabs.Trigger value="actions" className="rounded px-3 py-1.5 data-[state=active]:bg-muted">
            <TabLabel label="触发动作" count={actions.length} warn={tabHasIssue("actions")} />
          </Tabs.Trigger>
        </Tabs.List>
        {props.issues.length ? (
          <div className="flex flex-wrap items-center gap-2 border-b border-amber-200 bg-amber-50 px-4 py-2 text-xs text-amber-800">
            <span>待补齐</span>
            {props.issues.map((issue) => (
              <button
                key={`${issue.tab}-${issue.label}`}
                className="rounded-full border border-amber-200 bg-white px-2 py-0.5 hover:bg-amber-100"
                onClick={() => props.setTab(issue.tab)}
              >
                {issue.label}
              </button>
            ))}
          </div>
        ) : null}

        <Tabs.Content value="basic" className={tabContentClass}>
          <div className="scrollbar min-h-0 flex-1 overflow-auto p-4">
            <div className="grid content-start gap-4">
              <div className="grid gap-3 min-[1180px]:grid-cols-[minmax(0,1fr)_180px_160px]">
                <Field label="规则名称">
                  <input className="input" value={rule.name} onChange={(event) => props.updateEditing({ name: event.target.value })} />
                </Field>
                <Field label="触发时间">
                  <input className="input" type="time" value={rule.triggerTime ?? ""} onChange={(event) => props.updateEditing({ triggerTime: event.target.value })} />
                </Field>
                <Field label="冷却秒数">
                  <input className="input" type="number" min={0} value={rule.cooldownSeconds ?? 0} onChange={(event) => props.updateEditing({ cooldownSeconds: Math.max(0, Number(event.target.value) || 0) })} />
                </Field>
              </div>
              <Field label="触发应用">
                <AppPicker
                  apps={props.apps}
                  value={rule.appIdentifiers?.[0] ?? ""}
                  onChange={(value) => props.updateEditing({ appIdentifiers: value ? [value] : [] })}
                />
              </Field>
              <label className="flex items-center gap-3 text-sm">
                <Switch checked={rule.enabled ?? true} onCheckedChange={(enabled) => props.updateEditing({ enabled })} />
                启用规则
              </label>
            </div>
          </div>
        </Tabs.Content>

        <Tabs.Content value="match" className={tabContentClass}>
          <div className="scrollbar min-h-0 flex-1 overflow-auto p-4">
            <Header title="匹配条件" onAdd={addCondition} />
            <div className="grid content-start gap-3">
              {conditions.map((condition, index) => (
                <div key={index} className="rounded-md border border-border p-3">
                  <div className="mb-3 flex items-center justify-between gap-3">
                    <div className="text-xs font-medium text-subdued">条件 {index + 1}</div>
                    <RemoveButton onClick={() => props.updateEditing({ matchConditions: conditions.filter((_, itemIndex) => itemIndex !== index) })} />
                  </div>
                  <div className="grid grid-cols-1 gap-3 min-[1180px]:grid-cols-[minmax(120px,0.9fr)_minmax(116px,0.8fr)_minmax(150px,1fr)]">
                    <Field label="变量">
                      <select className="input" value={condition.variableName} onChange={(event) => props.updateCondition(index, { variableName: event.target.value })}>
                        {props.variableNames.map((name) => <option key={name}>{name}</option>)}
                      </select>
                    </Field>
                    <Field label="匹配方式">
                      <select className="input" value={condition.operatorType} onChange={(event) => props.updateCondition(index, { operatorType: event.target.value })}>
                        {operators.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                      </select>
                    </Field>
                    {["is_empty", "is_not_empty"].includes(condition.operatorType) ? (
                      <ReadonlyField label="匹配值" value="无需填写" />
                    ) : (
                      <Field label="匹配值">
                        <input className="input" value={condition.expectedValue ?? ""} onChange={(event) => props.updateCondition(index, { expectedValue: event.target.value })} />
                      </Field>
                    )}
                  </div>
                  <div className="mt-3 grid grid-cols-1 gap-3 min-[1180px]:grid-cols-[minmax(0,1fr)_120px]">
                    <ReadonlyField label="当前值" value={currentValueFor(condition.variableName)} multiline />
                    <label className="grid content-start gap-1 text-sm">
                      <span className="text-xs text-subdued">大小写</span>
                      <span className="flex h-9 items-center gap-2 rounded-md border border-border bg-white px-3 text-xs text-subdued">
                        <input type="checkbox" checked={condition.caseSensitive} onChange={(event) => props.updateCondition(index, { caseSensitive: event.target.checked })} />
                        Aa
                      </span>
                    </label>
                  </div>
                </div>
              ))}
              {!conditions.length ? <EmptyBlock label="尚未添加匹配条件" onClick={addCondition} /> : null}
            </div>
          </div>
        </Tabs.Content>

        <Tabs.Content value="variables" className={tabContentClass}>
          <div className="scrollbar min-h-0 flex-1 overflow-auto p-4">
            <Header title="自定义变量" onAdd={addVariable} />
            <div className="grid content-start gap-3">
              {variables.map((item, index) => (
                <div key={index} className="rounded-md border border-border p-3">
                  <div className="mb-3 flex items-center justify-between gap-3">
                    <div className="text-xs font-medium text-subdued">变量 {index + 1}</div>
                    <RemoveButton onClick={() => props.updateEditing({ variableExtractions: variables.filter((_, itemIndex) => itemIndex !== index) })} />
                  </div>
                  <div className="grid grid-cols-1 gap-3 min-[1180px]:grid-cols-2">
                    <Field label="变量名">
                      <input className="input" value={item.name} onChange={(event) => props.updateVariable(index, { name: event.target.value })} />
                    </Field>
                    <Field label="来源">
                      <select className="input" value={item.source} onChange={(event) => props.updateVariable(index, { source: event.target.value as VariableExtractionRule["source"] })}>
                        <option value="title">标题</option>
                        <option value="subtitle">副标题</option>
                        <option value="body">内容</option>
                      </select>
                    </Field>
                    <Field label="提取方式">
                      <select className="input" value={item.method} onChange={(event) => props.updateVariable(index, { method: event.target.value as VariableExtractionRule["method"] })}>
                        <option value="regex">正则</option>
                        <option value="between">截取</option>
                      </select>
                    </Field>
                    <Field label={item.method === "regex" ? "正则表达式" : "开始文本"}>
                      <input className="input" value={item.pattern ?? ""} onChange={(event) => props.updateVariable(index, { pattern: event.target.value })} />
                    </Field>
                    {item.method === "regex" ? (
                      <Field label="分组序号">
                        <input className="input" type="number" min={0} value={item.groupIndex ?? 1} onChange={(event) => props.updateVariable(index, { groupIndex: Number(event.target.value) })} />
                      </Field>
                    ) : (
                      <Field label="结束文本">
                        <input className="input" value={item.endPattern ?? ""} onChange={(event) => props.updateVariable(index, { endPattern: event.target.value })} />
                      </Field>
                    )}
                    <div className="min-[1180px]:col-span-2">
                      <ReadonlyField label="当前值" value={item.name.trim() ? currentValueFor(item.name.trim()) : ""} multiline />
                    </div>
                  </div>
                </div>
              ))}
              {!variables.length ? <EmptyBlock label="尚未添加自定义变量" onClick={addVariable} /> : null}
            </div>
          </div>
        </Tabs.Content>

        <Tabs.Content value="actions" className={tabContentClass}>
          <div className="scrollbar min-h-0 flex-1 overflow-auto p-4">
            <Header title="动作列表" onAdd={addAction} />
            <div className="mb-3 flex flex-wrap gap-2">
              {actionTemplates.map((template) => (
                <button
                  key={template.label}
                  className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border bg-white px-2.5 text-xs hover:bg-muted"
                  onClick={() => addAction(template.action)}
                >
                  <Plus size={13} />
                  {template.label}
                </button>
              ))}
            </div>
            <div className="grid content-start gap-3">
              {actions.map((action, index) => (
                <ActionEditor
                  key={index}
                  action={action}
                  apps={props.apps}
                  index={index}
                  variableNames={props.variableNames}
                  updateAction={props.updateAction}
                  updateActionParam={props.updateActionParam}
                  remove={() => props.updateEditing({ actions: actions.filter((_, itemIndex) => itemIndex !== index) })}
                />
              ))}
              {!actions.length ? <EmptyBlock label="尚未添加触发动作" onClick={addAction} /> : null}
            </div>
          </div>
        </Tabs.Content>
      </Tabs.Root>
    </section>
  );
}

function ActionEditor(props: {
  action: ActionConfig;
  apps: ApplicationInfo[];
  index: number;
  variableNames: string[];
  updateAction: (index: number, patch: Partial<ActionConfig>) => void;
  updateActionParam: (index: number, key: string, value: string) => void;
  remove: () => void;
}) {
  const { action, index } = props;
  const p = action.parameters;
  const [choosingScript, setChoosingScript] = useState(false);
  const [operationError, setOperationError] = useState("");
  const activeGroup = actionGroups.find((group) => (group.types as readonly string[]).includes(action.type)) ?? actionGroups[0];
  const setActionType = (type: string) => props.updateAction(index, { type, parameters: defaultParameters(type) });
  const isScriptAction = ["run_shell", "run_applescript", "run_javascript", "run_python"].includes(action.type);
  const scriptValueKey = action.type === "run_javascript" ? "code" : "script";
  const showTimeout = ["open_url", "open_app", "activate_app", "run_shell", "run_applescript", "run_javascript", "run_python", "http_request"].includes(action.type);

  function appendShellArgument(snippet: string) {
    const current = p[scriptValueKey] ?? "";
    props.updateActionParam(index, scriptValueKey, current ? `${current} ${snippet}` : snippet);
  }

  async function selectScriptFile() {
    setChoosingScript(true);
    setOperationError("");
    try {
      const path = await chooseScriptFile();
      if (!path) return;
      props.updateAction(index, {
        parameters: {
          ...action.parameters,
          ...scriptFileParameters(action.type, path),
        },
      });
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : String(err));
    } finally {
      setChoosingScript(false);
    }
  }

  return (
    <div className="rounded-md border border-border p-3">
      <div className="mb-3 grid gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex rounded-md border border-border bg-muted p-0.5">
            {actionGroups.map((group) => (
              <button
                key={group.id}
                className={`h-7 rounded px-2.5 text-xs ${activeGroup.id === group.id ? "bg-white shadow-soft" : "text-subdued"}`}
                onClick={() => setActionType(group.types[0])}
              >
                {group.label}
              </button>
            ))}
          </div>
          <button className="grid h-8 w-8 place-items-center rounded-md border border-border" onClick={props.remove} aria-label={`删除动作 ${index + 1}`} title="删除动作">
            <Trash2 size={14} />
          </button>
        </div>
        <div className="flex flex-wrap gap-2">
          {activeGroup.types.map((type) => (
            <button
              key={type}
              className={`h-8 rounded-md border px-3 text-sm ${action.type === type ? "border-accent bg-emerald-50 text-accent" : "border-border bg-white"}`}
              onClick={() => setActionType(type)}
            >
              {actionLabel(type)}
            </button>
          ))}
        </div>
        <div className="scrollbar flex max-h-20 flex-wrap gap-1 overflow-auto">
          {props.variableNames.map((name) => (
            <button key={name} className="rounded border border-border px-2 py-1 font-mono text-[11px]" onClick={() => navigator.clipboard?.writeText(`{{${name}}}`)}>
              <Copy size={11} className="mr-1 inline" />
              {name}
            </button>
          ))}
          {!props.variableNames.length ? <span className="text-xs text-subdued">暂无变量</span> : null}
        </div>
      </div>
      {action.type === "open_url" ? <TextParam label="URL" value={p.url ?? ""} onChange={(value) => props.updateActionParam(index, "url", value)} /> : null}
      {["open_app", "activate_app"].includes(action.type) ? (
        <Field label="应用">
          <AppPicker apps={props.apps} value={p.bundle_id ?? ""} onChange={(value) => props.updateActionParam(index, "bundle_id", value)} />
        </Field>
      ) : null}
      {action.type === "send_notification" ? (
        <div className="grid gap-2">
          <TextParam label="标题" value={p.title ?? ""} onChange={(value) => props.updateActionParam(index, "title", value)} />
          <TextParam label="内容" value={p.body ?? ""} onChange={(value) => props.updateActionParam(index, "body", value)} />
        </div>
      ) : null}
      {isScriptAction ? (
        <div className="grid gap-2">
          {action.type === "run_shell" ? (
            <div className="grid gap-2 min-[1180px]:grid-cols-[130px_180px_minmax(0,1fr)]">
              <Field label="Shell">
                <select className="input" value={p.shell ?? "bash"} onChange={(event) => props.updateActionParam(index, "shell", event.target.value)}>
                  <option value="bash">bash</option>
                  <option value="zsh">zsh</option>
                </select>
              </Field>
              <Field label="模式">
                <select className="input" value={p.shell_mode ?? "standard"} onChange={(event) => props.updateActionParam(index, "shell_mode", event.target.value)}>
                  <option value="standard">标准</option>
                  <option value="login">登录</option>
                  <option value="interactive">交互</option>
                  <option value="login_interactive">登录 + 交互</option>
                </select>
              </Field>
              <div className="flex items-end justify-end">
                <button className="button-secondary h-9" onClick={selectScriptFile} disabled={choosingScript}>
                  <FileCode2 size={14} />
                  选择脚本
                </button>
              </div>
            </div>
          ) : (
            <div className="flex justify-end">
              <button className="button-secondary h-9" onClick={selectScriptFile} disabled={choosingScript}>
                <FileCode2 size={14} />
                选择脚本
              </button>
            </div>
          )}
          <TextAreaParam
            label={scriptInputLabel(action.type)}
            value={p[scriptValueKey] ?? ""}
            onChange={(value) => props.updateActionParam(index, scriptValueKey, value)}
          />
          <TextParam label="工作目录" value={p.working_directory ?? ""} onChange={(value) => props.updateActionParam(index, "working_directory", value)} />
          <TextAreaParam label="环境变量 JSON" value={p.env_json ?? ""} onChange={(value) => props.updateActionParam(index, "env_json", value)} />
          {action.type === "run_shell" ? (
            <div className="flex flex-wrap gap-1">
              {[
                ["标题", "{{shell:title}}"],
                ["首个 URL", "{{shell:url}}"],
                ["全部 URL", "\"{{urls_join: }}\""],
                ["应用 ID", "{{shell:app_id}}"],
              ].map(([label, snippet]) => (
                <button
                  key={label}
                  className="rounded border border-border px-2 py-1 text-[11px] text-subdued hover:bg-muted"
                  onClick={() => appendShellArgument(snippet)}
                >
                  + {label}
                </button>
              ))}
            </div>
          ) : null}
          {operationError ? <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">{operationError}</div> : null}
        </div>
      ) : null}
      {action.type === "http_request" ? (
        <div className="grid gap-2">
          <TextParam label="URL" value={p.url ?? ""} onChange={(value) => props.updateActionParam(index, "url", value)} />
          <div className="grid gap-2 min-[1180px]:grid-cols-3">
            <TextParam label="方法" value={p.method ?? "GET"} onChange={(value) => props.updateActionParam(index, "method", value)} />
            <TextParam label="重试次数" value={p.retry_count ?? "0"} onChange={(value) => props.updateActionParam(index, "retry_count", value)} />
            <TextParam label="重试间隔秒" value={p.retry_interval_seconds ?? "1"} onChange={(value) => props.updateActionParam(index, "retry_interval_seconds", value)} />
          </div>
          <TextAreaParam label="Headers JSON" value={p.headers ?? ""} onChange={(value) => props.updateActionParam(index, "headers", value)} />
          <TextAreaParam label="Body" value={p.body ?? ""} onChange={(value) => props.updateActionParam(index, "body", value)} />
          <TextParam label="响应包含" value={p.response_contains ?? ""} onChange={(value) => props.updateActionParam(index, "response_contains", value)} />
          <TextParam label="工作目录" value={p.working_directory ?? ""} onChange={(value) => props.updateActionParam(index, "working_directory", value)} />
          <TextAreaParam label="环境变量 JSON" value={p.env_json ?? ""} onChange={(value) => props.updateActionParam(index, "env_json", value)} />
        </div>
      ) : null}
      {showTimeout ? (
        <div className="mt-2 max-w-[180px]">
          <TextParam label="超时秒数" value={p.timeout_seconds ?? ""} onChange={(value) => props.updateActionParam(index, "timeout_seconds", value)} />
        </div>
      ) : null}
    </div>
  );
}

export function PreviewPanel(props: {
  apps: ApplicationInfo[];
  appById: Map<string, ApplicationInfo>;
  notifications: NotificationRecord[];
  loading: boolean;
  refresh: () => void;
  selectedRecordId: number | null;
  setSelectedRecordId: (id: number) => void;
  preview: VariablePreview;
  query: string;
  setQuery: (value: string) => void;
  appFilter: string;
  setAppFilter: (value: string) => void;
  linkedApp: string;
}) {
  const variableRows = useMemo(
    () => props.selectedRecordId === null
      ? []
      : props.preview.displayNames.map((name) => ({
          name,
          value: props.preview.variables[name] ?? "",
        })),
    [props.preview.displayNames, props.preview.variables, props.selectedRecordId],
  );
  const listResetKey = `${props.query}\n${props.linkedApp || props.appFilter}`;

  return (
    <section className="flex min-h-0 flex-col rounded-lg border border-border bg-white shadow-soft">
      <Tabs.Root defaultValue="notifications" className="flex min-h-0 flex-1 flex-col">
        <Tabs.List className="flex border-b border-border p-2 text-sm">
          <Tabs.Trigger value="notifications" className="rounded px-3 py-1.5 data-[state=active]:bg-muted">
            <TabLabel label="通知预览" count={props.notifications.length} />
          </Tabs.Trigger>
          <Tabs.Trigger value="variables" className="rounded px-3 py-1.5 data-[state=active]:bg-muted">
            <TabLabel label="变量" count={variableRows.length} />
          </Tabs.Trigger>
        </Tabs.List>
        <Tabs.Content value="notifications" className={tabContentClass}>
          <div className="grid grid-cols-[minmax(0,1fr)_minmax(170px,220px)_36px] gap-2 border-b border-border p-3">
            <label className="relative">
              <Search className="pointer-events-none absolute left-2.5 top-2.5 text-subdued" size={15} />
              <input className="input input-search" value={props.query} onChange={(event) => props.setQuery(event.target.value)} />
            </label>
            <AppPicker
              apps={props.apps}
              value={props.linkedApp || props.appFilter}
              onChange={props.setAppFilter}
              disabled={!!props.linkedApp}
              allowEmpty
              emptyLabel="全部应用"
            />
            <button
              className="grid h-9 w-9 place-items-center rounded-md border border-border bg-white disabled:opacity-50"
              disabled={props.loading}
              onClick={props.refresh}
              aria-label="刷新通知预览"
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
            resetKey={listResetKey}
            emptyLabel={props.linkedApp ? "当前触发应用下暂无样本通知" : "暂无通知样本"}
          />
        </Tabs.Content>
        <Tabs.Content value="variables" className={tabContentClass}>
          <div className="scrollbar min-h-0 flex-1 overflow-auto p-3">
            <div className="grid content-start gap-2">
              {variableRows.map((row) => (
                <div key={row.name} className="grid grid-cols-[112px_minmax(0,1fr)] gap-3 rounded-md border border-border px-3 py-2 text-sm">
                  <div className="truncate font-mono text-xs text-subdued">{row.name}</div>
                  <div className="scrollbar min-w-0 max-h-32 overflow-auto whitespace-pre-wrap break-words text-slate-700">
                    {row.value.trim() ? row.value : <span className="text-subdued">空</span>}
                  </div>
                </div>
              ))}
              {!variableRows.length ? <EmptyBlock label="选择一条通知后展示变量" /> : null}
            </div>
          </div>
        </Tabs.Content>
      </Tabs.Root>
    </section>
  );
}

function actionLabel(type: string) {
  return actionTypes.find(([value]) => value === type)?.[1] ?? type;
}

function defaultParameters(type: string): Record<string, string> {
  switch (type) {
    case "open_url": return { url: "{{url}}" };
    case "send_notification": return { title: "{{title}}", body: "{{body}}" };
    case "http_request": return { method: "GET", url: "", retry_count: "0", retry_interval_seconds: "1" };
    case "run_shell": return { shell: "bash", shell_mode: "standard", script: "", timeout_seconds: "60" };
    case "run_javascript": return { code: "", timeout_seconds: "60" };
    case "run_applescript":
    case "run_python": return { script: "", timeout_seconds: "60" };
    default: return {};
  }
}

function cloneActionConfig(action: ActionConfig): ActionConfig {
  return {
    type: action.type,
    parameters: { ...action.parameters },
  };
}

function scriptInputLabel(type: string) {
  if (type === "run_shell") return "命令";
  if (type === "run_javascript") return "代码";
  return "脚本";
}

function scriptFileParameters(type: string, path: string): Record<string, string> {
  switch (type) {
    case "run_shell":
      return { script: shellScriptCommand(path) };
    case "run_python":
      return {
        script: [
          "import pathlib",
          "import runpy",
          "import sys",
          `_noticeflow_script = pathlib.Path(${pythonString(path)}).resolve()`,
          "sys.path.insert(0, str(_noticeflow_script.parent))",
          "runpy.run_path(str(_noticeflow_script), run_name=\"__main__\")",
        ].join("\n"),
      };
    case "run_applescript":
      return { script: `run script POSIX file ${appleScriptString(path)}` };
    case "run_javascript":
      return {
        code: [
          "const app = Application.currentApplication();",
          "app.includeStandardAdditions = true;",
          `app.doShellScript(${javascriptString(`/usr/bin/osascript -l JavaScript ${shellQuote(path)}`)});`,
        ].join("\n"),
      };
    default:
      return {};
  }
}

function shellScriptCommand(path: string) {
  const quoted = shellQuote(path);
  const lowerPath = path.toLowerCase();
  if (lowerPath.endsWith(".zsh")) return `zsh ${quoted}`;
  if (lowerPath.endsWith(".bash") || lowerPath.endsWith(".sh")) return `bash ${quoted}`;
  return quoted;
}

function shellQuote(value: string) {
  return `'${value.replace(/'/g, "'\\''")}'`;
}

function pythonString(value: string) {
  return JSON.stringify(value);
}

function javascriptString(value: string) {
  return JSON.stringify(value);
}

function appleScriptString(value: string) {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}
