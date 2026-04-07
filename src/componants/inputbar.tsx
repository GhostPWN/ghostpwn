import { createSignal } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { For, Show } from "solid-js";
import {
  SyntaxStyle,
  type MarkdownTableOptions,
  type ThemeTokenStyle,
} from "@opentui/core";
import { GhostLogo } from "./ghostlogo";
import { sendMessage, clearHistory, getProviderName } from "../ai";

type MessageRole = "user" | "assistant" | "error" | "tool";

interface Message {
  role: MessageRole;
  content: string;
}

const syntaxTheme = [
  { scope: ["comment"], style: { foreground: "#6b7280", italic: true } },
  { scope: ["string", "markup.raw", "markup.raw.block"], style: { foreground: "#a3e635" } },
  { scope: ["string.escape", "character.special"], style: { foreground: "#f59e0b" } },
  { scope: ["number", "boolean", "constant", "constant.builtin"], style: { foreground: "#fb7185" } },
  { scope: ["keyword", "keyword.import", "keyword.operator", "keyword.return"], style: { foreground: "#60a5fa", bold: true } },
  { scope: ["type", "type.builtin", "constructor"], style: { foreground: "#22d3ee" } },
  { scope: ["function", "function.call", "function.method", "function.method.call", "function.builtin"], style: { foreground: "#facc15" } },
  { scope: ["variable", "variable.parameter", "variable.member", "variable.builtin"], style: { foreground: "#e5e7eb" } },
  { scope: ["module", "module.builtin", "label"], style: { foreground: "#c4b5fd" } },
  { scope: ["operator", "punctuation.delimiter", "punctuation.bracket", "punctuation.special"], style: { foreground: "#94a3b8" } },
  { scope: ["markup.heading", "markup.heading.1", "markup.heading.2", "markup.heading.3", "markup.heading.4", "markup.heading.5", "markup.heading.6"], style: { foreground: "#f472b6", bold: true } },
  { scope: ["markup.strong"], style: { bold: true } },
  { scope: ["markup.italic"], style: { italic: true } },
  { scope: ["markup.link", "markup.link.label"], style: { foreground: "#c4b5fd", underline: true } },
  { scope: ["markup.link.url"], style: { foreground: "#22d3ee", underline: true } },
  { scope: ["markup.quote"], style: { foreground: "#9ca3af", italic: true } },
  { scope: ["markup.list", "markup.list.checked", "markup.list.unchecked"], style: { foreground: "#f9a8d4" } },
] satisfies ThemeTokenStyle[];

const syntaxStyle = SyntaxStyle.fromTheme(syntaxTheme);

const markdownTableOptions: MarkdownTableOptions = {
  widthMode: "full",
  wrapMode: "word",
  cellPadding: 1,
  borders: true,
  outerBorder: true,
  borderColor: "#3f3f46",
};

const TOOL_ARG_KEYS = {
  readFile: "path",
  listDirectory: "path",
  searchFiles: "pattern",
  grep: "pattern",
  runCommand: "command",
  fileInfo: "path",
} as const;

type KnownToolName = keyof typeof TOOL_ARG_KEYS;

function isKnownToolName(value: string): value is KnownToolName {
  return value in TOOL_ARG_KEYS;
}

function stringifyToolArg(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return "";
}

const SUPPORTED_RENDER_LANGUAGES = new Set([
  "javascript",
  "js",
  "typescript",
  "ts",
  "tsx",
  "jsx",
  "markdown",
  "md",
  "zig",
]);

const LANGUAGE_FALLBACKS: Record<string, string> = {
  python: "javascript",
  py: "javascript",
  bash: "javascript",
  sh: "javascript",
  zsh: "javascript",
  sql: "javascript",
  rust: "typescript",
  rs: "typescript",
  go: "typescript",
};

function normalizeRenderLanguage(language: string): string {
  const normalized = language.toLowerCase().trim();
  if (normalized.length === 0) return "typescript";
  if (SUPPORTED_RENDER_LANGUAGES.has(normalized)) return normalized;

  const fallback = LANGUAGE_FALLBACKS[normalized];
  if (fallback !== undefined) return fallback;

  return "typescript";
}

function normalizeExistingCodeFenceLanguages(content: string): string {
  return content.replace(/^```\s*([A-Za-z0-9_+-]+)([^\n]*)$/gm, (_full, lang: string) => {
    return `\`\`\`${normalizeRenderLanguage(lang)}`;
  });
}

function findNextNonEmptyLine(lines: string[], fromIndex: number): string | null {
  for (let i = fromIndex; i < lines.length; i += 1) {
    const line = lines[i];
    if (line !== undefined && line.trim().length > 0) {
      return line;
    }
  }
  return null;
}

function isLikelyCodeLine(line: string): boolean {
  const trimmed = line.trim();
  if (trimmed.length === 0 || trimmed.startsWith("```")) return false;

  if (/^\s{2,}\S/.test(line) || /^\t\S/.test(line)) return true;

  const patterns = [
    /^(def|class)\s+[A-Za-z_][\w]*/,
    /^(if|elif|else|for|while|try|except|finally)\b.*:?$/,
    /^return\b/,
    /^(const|let|var|function|interface|type|enum)\b/,
    /^(import|from)\b/,
    /^print\(/,
    /^[A-Za-z_][\w]*\s*=\s*.+/,
    /^['"]{3}/,
    /[{};]$/,
  ];

  return patterns.some((pattern) => pattern.test(trimmed));
}

function detectCodeLanguage(code: string): string {
  if (
    /(^|\n)\s*def\s+[A-Za-z_][\w]*\s*\(/.test(code) ||
    /(^|\n)\s*class\s+[A-Za-z_][\w]*/.test(code) ||
    /if\s+__name__\s*==\s*["']__main__["']/.test(code)
  ) {
    return "python";
  }

  if (
    /(^|\n)\s*(const|let|var)\s+/.test(code) ||
    /(^|\n)\s*function\s+/.test(code) ||
    /=>/.test(code)
  ) {
    return "typescript";
  }

  if (/^\s*#!/.test(code)) return "bash";
  if (/(^|\n)\s*(SELECT|INSERT|UPDATE|DELETE)\b/i.test(code)) return "sql";
  if (/(^|\n)\s*(fn|let\s+mut|impl|pub\s+fn)\b/.test(code)) return "rust";
  if (/(^|\n)\s*(package|func\s+[A-Za-z_][\w]*\(|import\s+\()/m.test(code)) {
    return "go";
  }

  return "text";
}

function ensureFencedCodeBlocks(content: string): string {
  const normalizedContent = normalizeExistingCodeFenceLanguages(content);
  if (normalizedContent.includes("```")) return normalizedContent;

  const lines = normalizedContent.split("\n");
  const output: string[] = [];

  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";

    if (!isLikelyCodeLine(line)) {
      output.push(line);
      i += 1;
      continue;
    }

    const codeLines: string[] = [];
    while (i < lines.length) {
      const current = lines[i] ?? "";

      if (current.trim().length === 0) {
        const nextNonEmpty = findNextNonEmptyLine(lines, i + 1);
        if (nextNonEmpty !== null && isLikelyCodeLine(nextNonEmpty)) {
          codeLines.push(current);
          i += 1;
          continue;
        }
        break;
      }

      if (!isLikelyCodeLine(current)) {
        break;
      }

      codeLines.push(current);
      i += 1;
    }

    const code = codeLines.join("\n");
    const hasStrongCodeSignal =
      /(^|\n)\s*(def|class|function|const|let|var|return)\b/.test(code) ||
      /(^|\n)\s*[A-Za-z_][\w]*\s*=\s*.+/.test(code);

    if (codeLines.length >= 2 || hasStrongCodeSignal) {
      const language = normalizeRenderLanguage(detectCodeLanguage(code));
      output.push(`\`\`\`${language}`);
      output.push(code);
      output.push("```");
    } else {
      output.push(...codeLines);
    }
  }

  return output.join("\n");
}

export function InputBar() {
  const [input, setInput] = createSignal("");
  const [messages, setMessages] = createSignal<Message[]>([]);
  const [isStreaming, setIsStreaming] = createSignal(false);
  const [streamingContent, setStreamingContent] = createSignal("");
  const [toolStatus, setToolStatus] = createSignal("");

  async function handleSubmit() {
    const text = input().trim();
    if (!text || isStreaming()) return;

    // Built-in commands
    if (text === "/clear") {
      clearHistory();
      setMessages([]);
      setInput("");
      return;
    }
    if (text === "/model") {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `Provider: ${getProviderName()}` },
      ]);
      setInput("");
      return;
    }
    if (text === "/quit") {
      process.exit(0);
    }

    // Send to LLM
    setMessages((prev) => [...prev, { role: "user", content: text }]);
    setInput("");
    setIsStreaming(true);
    setStreamingContent("");
    setToolStatus("");

    await sendMessage(text, {
      onText(delta) {
        setStreamingContent((prev) => prev + delta);
        setToolStatus("");
      },
      onToolCall(toolName, args) {
        // Flush any accumulated text before showing tool call
        const current = streamingContent();
        if (current) {
          setMessages((prev) => [
            ...prev,
            { role: "assistant", content: current },
          ]);
          setStreamingContent("");
        }

        const argSummary = formatToolArgs(toolName, args);
        setToolStatus(`${toolName}(${argSummary})`);
        setMessages((prev) => [
          ...prev,
          { role: "tool", content: `${toolName}(${argSummary})` },
        ]);
      },
      onToolResult(_toolName) {
        setToolStatus("");
      },
      onFinish(_text) {
        // Flush any remaining streaming content
        const remaining = streamingContent();
        if (remaining) {
          setMessages((prev) => [
            ...prev,
            { role: "assistant", content: remaining },
          ]);
        }
        setStreamingContent("");
        setToolStatus("");
        setIsStreaming(false);
      },
      onError(error) {
        setMessages((prev) => [...prev, { role: "error", content: error }]);
        setStreamingContent("");
        setToolStatus("");
        setIsStreaming(false);
      },
    });

    // Safety net in case onFinish didn't fire
    setIsStreaming(false);
  }

  useKeyboard((key) => {
    if (key.eventType !== "press") return;

    if (key.name === "return") {
      handleSubmit();
    } else if (key.name === "backspace") {
      setInput((prev) => prev.slice(0, -1));
    } else if (key.name === "c" && key.ctrl) {
      process.exit(0);
    } else if (key.name === "space" && !key.ctrl && !key.meta) {
      setInput((prev) => prev + " ");
    } else if (key.name.length === 1 && !key.ctrl && !key.meta) {
      setInput((prev) => prev + (key.shift ? key.name.toUpperCase() : key.name));
    }
  });

  return (
    <box flexDirection="column" flexGrow={1} width="100%">
      <box flexGrow={1} paddingX={2}>
        <Show
          when={messages().length > 0 || isStreaming()}
          fallback={
            <box
              flexDirection="column"
              flexGrow={1}
              width="100%"
              alignItems="center"
            >
              <box flexGrow={1} />
              <GhostLogo />
              <box flexGrow={1} />
            </box>
          }
        >
          <scrollbox
            flexDirection="column"
            flexGrow={1}
            overflow="hidden"
            stickyScroll={true}
          >
            <For each={messages()}>
              {(msg) => <MessageBubble msg={msg} />}
            </For>
            <Show when={isStreaming() && streamingContent()}>
              <box flexDirection="column" paddingBottom={1}>
                <markdown
                  content={ensureFencedCodeBlocks(streamingContent())}
                  syntaxStyle={syntaxStyle}
                  conceal={false}
                  concealCode={false}
                  tableOptions={markdownTableOptions}
                  streaming={true}
                />
              </box>
            </Show>
          </scrollbox>
        </Show>
      </box>
      <box borderStyle="single" borderColor="#6d28d9" paddingX={1} width="100%">
        <text fg={isStreaming() ? "#666666" : "#e9d5ff"}>
          {isStreaming()
            ? toolStatus()
              ? `  ${toolStatus()}`
              : "  thinking..."
            : "❯ " + input() + "█"}
        </text>
      </box>
    </box>
  );
}

function MessageBubble(props: { msg: Message }) {
  return (
    <Show
      when={props.msg.role === "user"}
      fallback={
        <Show
          when={props.msg.role === "tool"}
          fallback={
            <Show
              when={props.msg.role === "error"}
              fallback={
                <box flexDirection="column" paddingBottom={1}>
                  <markdown
                    content={ensureFencedCodeBlocks(props.msg.content)}
                    syntaxStyle={syntaxStyle}
                    conceal={false}
                    concealCode={false}
                    tableOptions={markdownTableOptions}
                  />
                </box>
              }
            >
              <box paddingBottom={1}>
                <text fg="#ef4444">{"✗ " + props.msg.content}</text>
              </box>
            </Show>
          }
        >
          <text fg="#666666">{props.msg.content}</text>
        </Show>
      }
    >
      <box paddingBottom={1}>
        <text fg="#c4b5fd">{"❯ " + props.msg.content}</text>
      </box>
    </Show>
  );
}

function formatToolArgs(
  toolName: string,
  args: Record<string, unknown>,
): string {
  if (isKnownToolName(toolName)) {
    const key = TOOL_ARG_KEYS[toolName];
    return stringifyToolArg(args[key]);
  }

  const fallback = JSON.stringify(args);
  return (fallback || "").slice(0, 60);
}
