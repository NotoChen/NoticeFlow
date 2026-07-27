import Anser from "anser";
import { invoke } from "@tauri-apps/api/core";
import type { CSSProperties, ReactNode } from "react";

type AnsiEntry = ReturnType<typeof Anser.ansiToJson>[number];

function entryStyle(entry: AnsiEntry): CSSProperties {
  let foreground = entry.fg_truecolor || entry.fg;
  let background = entry.bg_truecolor || entry.bg;
  const decorations = new Set(entry.decorations);
  if (decorations.has("reverse")) [foreground, background] = [background, foreground];
  return {
    color: foreground ? `rgb(${foreground})` : undefined,
    backgroundColor: background ? `rgb(${background})` : undefined,
    fontWeight: decorations.has("bold") ? 700 : undefined,
    fontStyle: decorations.has("italic") ? "italic" : undefined,
    opacity: decorations.has("dim") ? 0.6 : undefined,
    textDecoration: decorations.has("underline")
      ? "underline"
      : decorations.has("strikethrough")
        ? "line-through"
        : undefined,
    visibility: decorations.has("hidden") ? "hidden" : undefined,
  };
}

function linkedText(content: string, keyPrefix: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const urlPattern = /https?:\/\/[^\s<>"'`]+/g;
  let offset = 0;
  for (const match of content.matchAll(urlPattern)) {
    const index = match.index ?? 0;
    if (index > offset) nodes.push(content.slice(offset, index));
    const matched = match[0];
    const url = matched.replace(/[),.;:!?]+$/, "");
    const trailing = matched.slice(url.length);
    nodes.push(
      <a
        key={`${keyPrefix}-${index}`}
        href={url}
        target="_blank"
        rel="noreferrer"
        className="text-sky-400 underline underline-offset-2 hover:text-sky-300"
        onClick={(event) => {
          event.preventDefault();
          void invoke("open_external_url", { url }).catch((error) => console.error("Failed to open log URL", error));
        }}
      >
        {url}
      </a>,
    );
    if (trailing) nodes.push(trailing);
    offset = index + matched.length;
  }
  if (offset < content.length) nodes.push(content.slice(offset));
  return nodes;
}

export function AnsiOutput({ value }: { value: string }) {
  const entries = Anser.ansiToJson(value.replace(/\r\n/g, "\n").replace(/\r/g, "\n"), {
    json: true,
    remove_empty: true,
  });
  return (
    <pre className="scrollbar mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-md border border-slate-800 bg-slate-950 p-3 font-mono text-[11px] leading-5 text-slate-200">
      <code>
        {entries.map((entry, index) => (
          <span key={index} style={entryStyle(entry)}>
            {linkedText(entry.content, String(index))}
          </span>
        ))}
      </code>
    </pre>
  );
}
