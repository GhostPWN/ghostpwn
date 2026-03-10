import { createSignal } from "solid-js";
import { useKeyboard } from "@opentui/solid";
import { For, Show } from "solid-js";
import { GhostLogo } from "./ghostlogo";

export function InputBar() {
  const [input, setInput] = createSignal("");
  const [history, setHistory] = createSignal<string[]>([]);

  useKeyboard((key) => {
    if (key.eventType !== "press") return;

    if (key.name === "return" && input().trim()) {
      setHistory((prev) => [...prev, `❯ ${input()}`]);
      setInput("");
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
          when={history().length > 0}
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
          <scrollbox flexDirection="column" flexGrow={1} overflow="hidden">
            <For each={history()}>
              {(line) => <text fg="#c4b5fd">{line}</text>}
            </For>
          </scrollbox>
        </Show>
      </box>
      <box borderStyle="single" borderColor="#6d28d9" paddingX={1} width="100%">
        <text fg="#e9d5ff">{"❯ " + input() + "█"}</text>
      </box>
    </box>
  );
}
