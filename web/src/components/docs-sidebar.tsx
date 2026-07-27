"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Menu } from "lucide-react";
import { useRef } from "react";
import { NAV } from "@/lib/docs-nav";
import { cn } from "@/lib/utils";

export function DocsSidebar() {
  const pathname = usePathname();

  return (
    <nav className="flex flex-col gap-6 text-sm">
      {NAV.map((section, i) => (
        <div key={section.title ?? i} className="flex flex-col gap-1">
          {section.title && (
            <p className="mb-1 px-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              {section.title}
            </p>
          )}
          {section.items.map((item) => {
            const active = pathname === item.href;
            return (
              <Link
                key={item.href}
                href={item.href}
                className={cn(
                  "rounded-md px-2 py-1.5 transition-colors",
                  active
                    ? "bg-accent font-medium text-foreground"
                    : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
                )}
              >
                {item.title}
              </Link>
            );
          })}
        </div>
      ))}
    </nav>
  );
}

export function MobileDocsMenu() {
  const details = useRef<HTMLDetailsElement>(null);

  return (
    <details ref={details} className="relative md:hidden">
      <summary className="flex size-8 cursor-pointer list-none items-center justify-center rounded-md border hover:bg-accent [&::-webkit-details-marker]:hidden">
        <Menu className="size-4" />
        <span className="sr-only">Open documentation menu</span>
      </summary>
      <div
        className="fixed inset-x-4 top-24 max-h-[calc(100vh-7rem)] overflow-y-auto rounded-xl border bg-background p-4 shadow-lg"
        onClick={() => details.current?.removeAttribute("open")}
      >
        <DocsSidebar />
      </div>
    </details>
  );
}
