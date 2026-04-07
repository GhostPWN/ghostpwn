import { createProviderRegistry } from "ai";
import { anthropic } from "@ai-sdk/anthropic";
import { openai } from "@ai-sdk/openai";
import { google } from "@ai-sdk/google";

const registry = createProviderRegistry({
  anthropic,
  openai,
  google,
});

type Provider = "anthropic" | "openai" | "google";

const DEFAULT_PROVIDER: Provider = "google";

const DEFAULT_MODELS: Record<Provider, string> = {
  anthropic: "claude-sonnet-4-6",
  openai: "gpt-5.4",
  google: "gemini-3.1-flash-lite-preview",
};

const API_KEY_VARS: Record<Provider, string> = {
  anthropic: "ANTHROPIC_API_KEY",
  openai: "OPENAI_API_KEY",
  google: "GOOGLE_GENERATIVE_AI_API_KEY",
};

function isProvider(value: string): value is Provider {
  return value === "anthropic" || value === "openai" || value === "google";
}

function getProvider(): Provider {
  const configuredProvider = process.env["GHOSTPWN_PROVIDER"];
  if (!configuredProvider) {
    return DEFAULT_PROVIDER;
  }

  return isProvider(configuredProvider) ? configuredProvider : DEFAULT_PROVIDER;
}

function getModelId(provider: Provider): string {
  return process.env["GHOSTPWN_MODEL"] || DEFAULT_MODELS[provider];
}

export function getModel() {
  const provider = getProvider();
  const modelId = getModelId(provider);

  const keyVar = API_KEY_VARS[provider];
  if (keyVar && !process.env[keyVar]) {
    throw new Error(
      `No API key set for ${provider}. Set ${keyVar} in your .env file.`,
    );
  }

  return registry.languageModel(`${provider}:${modelId}`);
}

export function getProviderName(): string {
  const provider = getProvider();
  return `${provider} / ${getModelId(provider)}`;
}
