import { createSignal } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { For, Show } from "solid-js";
import { SyntaxStyle } from "@opentui/core";
import { GhostLogo } from "./ghostlogo";
import { sendMessage, clearHistory, getProviderName } from "../ai";

interface Message {
  role: "user" | "assistant" | "error";
  content: string;
}

const syntaxStyle = SyntaxStyle.create();

export function InputBar() {
  const [input, setInput] = createSignal("");
  const [messages, setMessages] = createSignal<Message[]>([]);
  const [isStreaming, setIsStreaming] = createSignal(false);
  const [streamingContent, setStreamingContent] = createSignal("");

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

    try {
      const { textStream, response } = sendMessage(text);

      for await (const chunk of textStream) {
        setStreamingContent((prev) => prev + chunk);
      }

      await response;
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: streamingContent() },
      ]);
    } catch (err) {
      const errorMsg =
        err instanceof Error ? err.message : "An unknown error occurred";
      setMessages((prev) => [...prev, { role: "error", content: errorMsg }]);
    } finally {
      setStreamingContent("");
      setIsStreaming(false);
    }
  }

  useKeyboard((key) => {
    if (key.eventType !== "press") return;

    if (key.name === "return") {
      handleSubmit();
    } else if (key.name === "backspace") {
      setInput((prev) => prev.slice(0, -1));
    } else if (key.name === "c" && key.ctrl) {
      process.exit(0);
    } else if (key.name.length === 1 && !key.ctrl && !key.meta) {
      setInput((prev) => prev + key.name);
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
              {(msg) => (
                <Show
                  when={msg.role === "error"}
                  fallback={
                    <Show
                      when={msg.role === "user"}
                      fallback={
                        <box flexDirection="column" paddingBottom={1}>
                          <markdown
                            content={msg.content}
                            syntaxStyle={syntaxStyle}
                          />
                        </box>
                      }
                    >
                      <box paddingBottom={1}>
                        <text fg="#c4b5fd">{"❯ " + msg.content}</text>
                      </box>
                    </Show>
                  }
                >
                  <box paddingBottom={1}>
                    <text fg="#ef4444">{"✗ " + msg.content}</text>
                  </box>
                </Show>
              )}
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
          {isStreaming() ? "  thinking..." : "❯ " + input() + "█"}
        </text>
      </box>
    </box>
  );
}
