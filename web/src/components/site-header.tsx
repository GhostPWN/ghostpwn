"use client";

import Image from "next/image";
import Link from "next/link";
import { MorphIcon, type IconNode } from "morphicons/react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { ThemeToggle } from "@/components/theme-toggle";
import { asset } from "@/lib/asset";
import { cn } from "@/lib/utils";

const DOCS_URL = "/docs";
const GITHUB_URL = "https://github.com/GhostPWN/ghostpwn";

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

// mobileMenu: content shown below md inside the bar, behind a menu toggle.
export function SiteHeader({
  mobileMenu,
  showDocsLink = true,
}: {
  mobileMenu?: React.ReactNode;
  showDocsLink?: boolean;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const bar = useRef<HTMLDivElement>(null);
  const toggle = useRef<HTMLButtonElement>(null);

  // Escape closes and returns focus to the toggle; a press outside the bar closes.
  useEffect(() => {
    if (!menuOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setMenuOpen(false);
      toggle.current?.focus();
    };
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !bar.current?.contains(event.target)) setMenuOpen(false);
    };
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [menuOpen]);

  return (
    // Below md the header keeps the closed bar's height, so the open menu
    // overlays the page instead of pushing it down (as binje's fixed nav does).
    <header className="sticky top-0 z-20 h-[4.625rem] px-4 pt-4 sm:h-[4.375rem] md:h-auto">
      <div
        ref={bar}
        className="mx-auto w-full max-w-6xl rounded-2xl border bg-background/70 px-5 py-3 shadow-sm backdrop-blur-md"
      >
        <div className="flex items-center justify-between">
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
            {mobileMenu && (
              <button
                ref={toggle}
                type="button"
                aria-expanded={menuOpen}
                aria-controls="mobile-menu"
                aria-label={menuOpen ? "Close menu" : "Open menu"}
                onClick={() => setMenuOpen((open) => !open)}
                className="flex size-8 cursor-pointer items-center justify-center rounded-md border outline-none hover:bg-accent sm:size-7 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background md:hidden"
              >
                <MorphIcon
                  icon={menuOpen ? CLOSE_ICON : MENU_ICON}
                  className="size-4"
                  reducedMotion="user"
                />
              </button>
            )}
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
        {mobileMenu && (
          // Same motion as binje's mobile menu: the bar grows to hold the menu,
          // the panel fades in, and each link fades and slides down into place.
          <div
            id="mobile-menu"
            inert={!menuOpen}
            onClick={(event) => {
              if (event.target instanceof Element && event.target.closest("a")) setMenuOpen(false);
            }}
            className={cn(
              "grid overflow-hidden transition-[opacity,transform] duration-300 ease-out motion-reduce:transition-none md:hidden",
              menuOpen
                ? "grid-rows-[1fr] translate-y-0 opacity-100"
                : "pointer-events-none grid-rows-[0fr] -translate-y-2 opacity-0",
            )}
          >
            <div className="min-h-0 overflow-hidden">
              <div
                className={cn(
                  "max-h-[calc(100dvh-8rem)] overflow-y-auto pt-4 pb-1 [&_a]:transition [&_a]:duration-200 motion-reduce:[&_a]:transition-none",
                  menuOpen
                    ? "[&_a]:translate-y-0 [&_a]:opacity-100"
                    : "[&_a]:-translate-y-1 [&_a]:opacity-0",
                )}
              >
                {mobileMenu}
              </div>
            </div>
          </div>
        )}
      </div>
    </header>
  );
}
