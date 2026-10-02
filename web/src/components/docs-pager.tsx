import Link from "next/link";
import { NAV } from "@/lib/docs-nav";

const PAGES = NAV.flatMap((section) => section.items);

const LINK_CLASS =
  "flex flex-col gap-1 rounded-lg border p-4 outline-none transition-colors hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring";

// Previous and next pages follow the sidebar order.
export function DocsPager({ href }: { href: string }) {
  const index = PAGES.findIndex((page) => page.href === href);
  if (index === -1) return null;
  const prev = PAGES[index - 1];
  const next = PAGES[index + 1];

  return (
    <nav
      aria-label="Previous and next pages"
      className="mt-12 grid gap-3 border-t pt-6 sm:grid-cols-2"
    >
      {prev && (
        <Link href={prev.href} className={LINK_CLASS}>
          <span className="text-xs text-muted-foreground">Previous</span>
          <span className="font-medium">{prev.title}</span>
        </Link>
      )}
      {next && (
        <Link href={next.href} className={`${LINK_CLASS} text-right sm:col-start-2`}>
          <span className="text-xs text-muted-foreground">Next</span>
          <span className="font-medium">{next.title}</span>
        </Link>
      )}
    </nav>
  );
}
