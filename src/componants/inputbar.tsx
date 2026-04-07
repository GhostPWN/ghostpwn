import { createSignal } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { For, Show } from "solid-js";
import { SyntaxStyle } from "@opentui/core";
import { GhostLogo } from "./ghostlogo";
import { sendMessage, clearHistory, getProviderName } from "../ai";

type MessageRole = "user" | "assistant" | "error" | "tool";

interface Message {
  role: MessageRole;
  content: string;
}

const syntaxStyle = SyntaxStyle.create();

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
        setToolStatus(`⚡ ${toolName}(${argSummary})`);
        setMessages((prev) => [
          ...prev,
          { role: "tool", content: `⚡ ${toolName}(${argSummary})` },
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
                  content={streamingContent()}
                  syntaxStyle={syntaxStyle}
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
                    content={props.msg.content}
                    syntaxStyle={syntaxStyle}
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
  switch (toolName) {
    case "readFile":
      return String(args["path"] || "");
    case "listDirectory":
      return String(args["path"] || "");
    case "searchFiles":
      return String(args["pattern"] || "");
    case "grep":
      return String(args["pattern"] || "");
    case "runCommand":
      return String(args["command"] || "");
    case "fileInfo":
      return String(args["path"] || "");
    default:
      return JSON.stringify(args).slice(0, 60);
  }
}
