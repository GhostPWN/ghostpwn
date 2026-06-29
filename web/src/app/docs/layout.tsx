import Image from "next/image";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { DocsSidebar } from "@/components/docs-sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { asset } from "@/lib/asset";

const GITHUB_URL = "https://github.com/GhostPWN/ghostpwn";

export default function DocsLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-h-full flex-col">
      <header className="sticky top-0 z-20 px-4 pt-4">
        <div className="mx-auto flex w-full max-w-6xl items-center justify-between rounded-2xl border bg-background/70 px-5 py-3 shadow-sm backdrop-blur-md">
          <Link href="/" className="flex items-center gap-2.5">
            <Image src={asset("/ghostpwn-logo.svg")} alt="GhostPWN" width={28} height={28} />
            <span className="font-semibold tracking-tight">GhostPWN</span>
          </Link>
          <nav className="flex items-center gap-2">
            <ThemeToggle />
            <Button variant="outline" size="sm" render={<a href={GITHUB_URL} />}>
              GitHub
            </Button>
          </nav>
        </div>
      </header>

      <div className="mx-auto flex w-full max-w-6xl flex-1 gap-10 px-6">
        <aside className="hidden w-56 shrink-0 py-10 md:block">
          <div className="sticky top-24">
            <DocsSidebar />
          </div>
        </aside>
        <main className="min-w-0 flex-1 py-10">{children}</main>
      </div>
    </div>
  );
}
