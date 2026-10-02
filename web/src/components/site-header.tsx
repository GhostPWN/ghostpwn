import Image from "next/image";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { ThemeToggle } from "@/components/theme-toggle";
import { asset } from "@/lib/asset";

const DOCS_URL = "/docs";
const GITHUB_URL = "https://github.com/GhostPWN/ghostpwn";

// children: extra nav items rendered first (e.g. the mobile docs menu).
export function SiteHeader({
  children,
  showDocsLink = true,
}: {
  children?: React.ReactNode;
  showDocsLink?: boolean;
}) {
  return (
    <header className="sticky top-0 z-20 px-4 pt-4">
      <div className="mx-auto flex w-full max-w-6xl items-center justify-between rounded-2xl border bg-background/70 px-5 py-3 shadow-sm backdrop-blur-md">
        <Link
          href="/"
          className="flex items-center gap-2.5 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Image
            src={asset("/ghostpwn-logo.svg")}
            alt=""
            width={28}
            height={28}
          />
          <span className="font-semibold tracking-tight">GhostPWN</span>
        </Link>
        <nav aria-label="Primary" className="flex items-center gap-2">
          {children}
          <ThemeToggle />
          {showDocsLink && (
            <Button variant="ghost" size="sm" render={<Link href={DOCS_URL} />}>
              Docs
            </Button>
          )}
          <Button variant="outline" size="sm" render={<a href={GITHUB_URL} />}>
            GitHub
          </Button>
        </nav>
      </div>
    </header>
  );
}
