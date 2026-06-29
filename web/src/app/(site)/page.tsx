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
import { ThemeToggle } from "@/components/theme-toggle";
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
      "Read, list, search, diff, edit, and run commands, all bounded to a configured workspace root.",
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
/clear    # reset conversation
/quit     # exit the TUI`;

export default function Home() {
  return (
    <main className="flex flex-1 flex-col">
      {/* Nav */}
      <header className="sticky top-0 z-20 px-4 pt-4">
        <div className="mx-auto flex w-full max-w-6xl items-center justify-between rounded-2xl border bg-background/70 px-5 py-3 shadow-sm backdrop-blur-md">
          <div className="flex items-center gap-2.5">
            <Image
              src={asset("/ghostpwn-logo.svg")}
              alt="GhostPWN"
              width={28}
              height={28}
            />
            <span className="font-semibold tracking-tight">GhostPWN</span>
          </div>
          <nav className="flex items-center gap-2">
            <ThemeToggle />
            <Button variant="ghost" size="sm" render={<Link href={DOCS_URL} />}>
              Docs
            </Button>
            <Button
              variant="outline"
              size="sm"
              render={<a href={GITHUB_URL} />}
            >
              GitHub
            </Button>
          </nav>
        </div>
      </header>

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
        <h1 className="font-heading text-5xl font-bold tracking-tight sm:text-6xl">
          Autonomous penetration testing agent
        </h1>
        <p className="mt-6 max-w-2xl text-lg text-muted-foreground">
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
        <h2 className="mb-10 text-center font-heading text-3xl font-bold tracking-tight">
          Built for offensive security research
        </h2>
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {FEATURES.map((feature) => (
            <Card key={feature.title}>
              <CardHeader>
                <CardTitle>{feature.title}</CardTitle>
                <CardDescription>{feature.description}</CardDescription>
              </CardHeader>
            </Card>
          ))}
        </div>
      </section>

      {/* Commands */}
      <section className="mx-auto w-full max-w-3xl px-6 pb-24">
        <h2 className="mb-6 text-center font-heading text-3xl font-bold tracking-tight">
          In-session commands
        </h2>
        <CodeBlock code={COMMANDS} lang="bash" />
      </section>

      {/* Footer */}
      <footer className="mt-auto border-t">
        <div className="mx-auto flex w-full max-w-5xl flex-col items-center justify-between gap-4 px-6 py-8 text-sm text-muted-foreground sm:flex-row">
          <span>© {new Date().getFullYear()} GhostPWN · MIT</span>
          <nav className="flex items-center gap-5">
            <Link href={DOCS_URL} className="hover:text-foreground">
              Documentation
            </Link>
            <a href={GITHUB_URL} className="hover:text-foreground">
              GitHub
            </a>
          </nav>
        </div>
      </footer>
    </main>
  );
}
