import { streamText } from "ai";
import type { ModelMessage } from "@ai-sdk/provider-utils";
import { getModel } from "./provider";

const SYSTEM_PROMPT = `You are GhostPWN, an autonomous web penetration testing assistant for academic security research. You help analyze web application vulnerabilities. Be precise, technical, and security-focused.`;

let history: ModelMessage[] = [];

export function clearHistory() {
  history = [];
}

export function sendMessage(text: string) {
  history.push({ role: "user", content: text });

  const result = streamText({
    model: getModel(),
    system: SYSTEM_PROMPT,
    messages: history,
  });

  return {
    textStream: result.textStream,
    response: result.response.then((res) => {
      history.push(...res.messages);
    }),
  };
}
