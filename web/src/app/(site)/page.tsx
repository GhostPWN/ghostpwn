import { SiteFooter } from "@/components/site-footer";
import { SiteHeader } from "@/components/site-header";
import Image from "next/image";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { CodeBlock } from "@/components/code-block";
import { asset } from "@/lib/asset";

const DOCS_URL = "/docs";
const GITHUB_URL = "https://github.com/GhostPWN/ghostpwn";

const FEATURES = [
  {
    title: "Terminal interface",
    description:
      "A ratatui + crossterm TUI with streaming output, auto-scroll, and transcript controls.",
  },
  {
    title: "Multi-provider",
    description:
      "OpenAI, Anthropic, Google, GitHub Copilot, and local Ollama, with in-session model switching.",
  },
  {
    title: "Secure key storage",
    description:
      "Persistent API keys via the OS keychain, with environment and local state-file fallbacks.",
  },
  {
    title: "Local tools",
    description:
      "Read, list, search, diff, and edit inside a configured workspace. Shell commands require approval but are not sandboxed.",
  },
  {
    title: "OAuth providers",
    description:
      "GitHub Copilot device authorization and Codex ChatGPT/Codex OAuth browser login.",
  },
  {
    title: "Clear architecture",
    description:
      "A JSON-first agent loop, vendor provider adapters, and workspace-safe tool implementations.",
  },
];

const INSTALL = `# macOS
brew install GhostPWN/tap/ghostpwn

# Linux / Windows
cargo install --git https://github.com/GhostPWN/ghostpwn

ghostpwn`;

const COMMANDS = `/help     # show all commands
/model    # provider + model selector
/audit    # read-only workspace security audit
/audit --fix # audit and apply approved fixes
/clear    # reset conversation
/quit     # exit the TUI`;

export default function Home() {
  return (
    <>
      <SiteHeader />

      <main id="main-content" tabIndex={-1} className="flex flex-1 flex-col">
        {/* Hero */}
        <section className="mx-auto flex w-full max-w-5xl flex-col items-center px-6 py-24 text-center">
          <Image
            src={asset("/ghostpwn-logo.svg")}
            alt="GhostPWN logo"
            width={96}
            height={96}
            className="mb-8"
            loading="eager"
          />
          <Badge variant="secondary" className="mb-6">
            Rust · ratatui · Multi-provider LLM
          </Badge>
          <h1 className="text-balance font-heading text-5xl font-bold sm:text-6xl">
            Autonomous penetration testing agent
          </h1>
          <p className="mt-6 max-w-2xl text-pretty text-lg text-muted-foreground">
            GhostPWN is a Rust terminal assistant for offensive security research.
            It streams from multiple LLM providers and runs local tools inside a
            workspace boundary.
          </p>
          <div className="mt-10 flex flex-col gap-3 sm:flex-row">
            <Button size="xl" render={<Link href={DOCS_URL} />}>
              View Documentation
            </Button>
            <Button size="xl" variant="outline" render={<a href={GITHUB_URL} />}>
              Star on GitHub
            </Button>
          </div>
        </section>

        {/* Install */}
        <section className="mx-auto w-full max-w-3xl px-6 pb-24">
          <CodeBlock code={INSTALL} lang="bash" />
        </section>

        {/* Features */}
        <section className="mx-auto w-full max-w-5xl px-6 pb-24">
          <h2 className="mb-10 text-balance text-center font-heading text-3xl font-bold">
            Built for offensive security research
          </h2>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {FEATURES.map((feature) => (
              <Card key={feature.title}>
                <CardHeader>
                  <CardTitle render={<h3 />}>{feature.title}</CardTitle>
                  <CardDescription className="text-pretty">
                    {feature.description}
                  </CardDescription>
                </CardHeader>
              </Card>
            ))}
          </div>
        </section>

        {/* Commands */}
        <section className="mx-auto w-full max-w-3xl px-6 pb-24">
          <h2 className="mb-6 text-balance text-center font-heading text-3xl font-bold">
            In-session commands
          </h2>
          <CodeBlock code={COMMANDS} lang="bash" />
        </section>
      </main>

      {/* Footer */}
      <SiteFooter />
    </>
  );
}
