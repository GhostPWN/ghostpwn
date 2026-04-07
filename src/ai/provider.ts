import { createProviderRegistry } from "ai";
import { anthropic } from "@ai-sdk/anthropic";
import { openai } from "@ai-sdk/openai";
import { google } from "@ai-sdk/google";

const registry = createProviderRegistry({
  anthropic,
  openai,
  google,
});

const DEFAULT_MODELS: Record<string, string> = {
  anthropic: "claude-sonnet-4-6",
  openai: "gpt-5.4",
  google: "gemini-3.1-flash-lite-preview",
};

const API_KEY_VARS: Record<string, string> = {
  anthropic: "ANTHROPIC_API_KEY",
  openai: "OPENAI_API_KEY",
  google: "GOOGLE_GENERATIVE_AI_API_KEY",
};

function getProvider(): string {
  return process.env["GHOSTPWN_PROVIDER"] || "google";
}

function getModelId(): string {
  const provider = getProvider();
  return (
    process.env["GHOSTPWN_MODEL"] ||
    DEFAULT_MODELS[provider] ||
    "gemini-3.1-flash-lite-preview"
  );
}

export function getModel() {
  const provider = getProvider();
  const modelId = getModelId();

  const keyVar = API_KEY_VARS[provider];
  if (keyVar && !process.env[keyVar]) {
    throw new Error(
      `No API key set for ${provider}. Set ${keyVar} in your .env file.`,
    );
  }

  return registry.languageModel(`${provider}:${modelId}` as any);
}

export function getProviderName(): string {
  return `${getProvider()} / ${getModelId()}`;
}
