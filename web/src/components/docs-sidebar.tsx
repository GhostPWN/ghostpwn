"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { MorphIcon, type IconNode } from "morphicons/react";
import { useEffect, useRef, useState } from "react";
import { NAV } from "@/lib/docs-nav";
import { cn } from "@/lib/utils";

export function DocsSidebar() {
  const pathname = usePathname();
  const currentPath = pathname.replace(/\/$/, "") || "/";

  return (
    <nav aria-label="Documentation" className="flex flex-col gap-6 text-sm">
      {NAV.map((section, i) => (
        <div key={section.title ?? i} className="flex flex-col gap-1">
          {section.title && (
            <p className="mb-1 px-2 text-xs font-semibold uppercase text-muted-foreground">
              {section.title}
            </p>
          )}
          {section.items.map((item) => {
            const active = currentPath === item.href;
            return (
              <Link
                key={item.href}
                href={item.href}
                aria-current={active ? "page" : undefined}
                className={cn(
                  "rounded-md px-2 py-1.5 transition-colors",
                  active
                    ? "bg-accent font-medium text-brand"
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

// Lucide's menu and x as icon data, so morphicons can morph between them.
const MENU_ICON: IconNode = [
  ["path", { d: "M4 5h16" }],
  ["path", { d: "M4 12h16" }],
  ["path", { d: "M4 19h16" }],
];
const CLOSE_ICON: IconNode = [
  ["path", { d: "M18 6 6 18" }],
  ["path", { d: "m6 6 12 12" }],
];

export function MobileDocsMenu() {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);

  // Escape closes and returns focus to the toggle; a press outside closes.
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setOpen(false);
      button.current?.focus();
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      if (button.current?.contains(event.target) || panel.current?.contains(event.target)) return;
      setOpen(false);
    };
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open]);

  return (
    <div className="md:hidden">
      <button
        ref={button}
        type="button"
        aria-expanded={open}
        aria-controls="mobile-docs-menu"
        aria-label={open ? "Close documentation menu" : "Open documentation menu"}
        onClick={() => setOpen((value) => !value)}
        className="flex size-8 cursor-pointer items-center justify-center rounded-md border outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background"
      >
        <MorphIcon icon={open ? CLOSE_ICON : MENU_ICON} className="size-4" reducedMotion="user" />
      </button>
      {/* Stays mounted so closing can animate; inert keeps it out of focus and the a11y tree. */}
      <div
        ref={panel}
        id="mobile-docs-menu"
        inert={!open}
        onClick={(event) => {
          if (event.target instanceof Element && event.target.closest("a")) setOpen(false);
        }}
        className={cn(
          "fixed inset-x-4 top-24 max-h-[calc(100dvh-7rem)] overflow-y-auto rounded-lg border bg-background p-4 shadow-lg transition-[opacity,translate] duration-300 ease-out motion-reduce:transition-none",
          open ? "translate-y-0 opacity-100" : "pointer-events-none -translate-y-2 opacity-0",
        )}
      >
        <DocsSidebar />
      </div>
    </div>
  );
}
