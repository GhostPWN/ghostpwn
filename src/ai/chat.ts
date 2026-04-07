import { streamText, stepCountIs } from "ai";
import type { ModelMessage } from "@ai-sdk/provider-utils";
import { getModel } from "./provider";
import { agentTools } from "./tools";

const SYSTEM_PROMPT = `You are GhostPWN, an autonomous web penetration testing assistant for academic security research. You help analyze web application vulnerabilities. Be precise, technical, and security-focused.

You have tools to explore the codebase and run commands. Use them proactively to understand the project before answering questions about it.`;

let history: ModelMessage[] = [];

export function clearHistory() {
  history = [];
}

export interface StreamCallbacks {
  onText: (delta: string) => void;
  onToolCall: (toolName: string, args: Record<string, unknown>) => void;
  onToolResult: (toolName: string) => void;
  onFinish: (text: string) => void;
  onError: (error: string) => void;
}

export async function sendMessage(text: string, callbacks: StreamCallbacks) {
  history.push({ role: "user", content: text });

  try {
    const result = streamText({
      model: getModel(),
      system: SYSTEM_PROMPT,
      messages: history,
      tools: agentTools,
      stopWhen: stepCountIs(15),
      onError({ error }) {
        callbacks.onError(
          error instanceof Error ? error.message : String(error),
        );
      },
    });

    let fullText = "";

    for await (const part of result.fullStream) {
      switch (part.type) {
        case "text-delta":
          fullText += part.text;
          callbacks.onText(part.text);
          break;
        case "tool-call":
          callbacks.onToolCall(
            part.toolName,
            part.input as Record<string, unknown>,
          );
          break;
        case "tool-result":
          callbacks.onToolResult(part.toolName);
          break;
        case "error":
          callbacks.onError(String(part.error));
          break;
      }
    }

    const response = await result.response;
    history.push(...response.messages);

    callbacks.onFinish(fullText);
  } catch (err) {
    callbacks.onError(
      err instanceof Error ? err.message : "An unknown error occurred",
    );
  }
}
