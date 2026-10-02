"use client";

import * as React from "react";
import { Moon, Sun } from "lucide-react";
import { MorphIcon, type IconNode } from "morphicons/react";
import { useTheme } from "next-themes";
import { Button } from "@/components/ui/button";

// Lucide's sun and moon as icon data, so morphicons can morph between them.
const SUN_ICON: IconNode = [
  ["circle", { cx: "12", cy: "12", r: "4" }],
  ["path", { d: "M12 2v2" }],
  ["path", { d: "M12 20v2" }],
  ["path", { d: "m4.93 4.93 1.41 1.41" }],
  ["path", { d: "m17.66 17.66 1.41 1.41" }],
  ["path", { d: "M2 12h2" }],
  ["path", { d: "M20 12h2" }],
  ["path", { d: "m6.34 17.66-1.41 1.41" }],
  ["path", { d: "m19.07 4.93-1.41 1.41" }],
];
const MOON_ICON: IconNode = [
  [
    "path",
    {
      d: "M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401",
    },
  ],
];

const subscribe = () => () => {};
const getSnapshot = () => true;
const getServerSnapshot = () => false;

export function ThemeToggle() {
  const { resolvedTheme, setTheme } = useTheme();
  const mounted = React.useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  return (
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label="Toggle theme"
      onClick={() => setTheme(resolvedTheme === "dark" ? "light" : "dark")}
    >
      {mounted ? (
        // First client render shows the current icon statically; later toggles morph.
        <MorphIcon
          icon={resolvedTheme === "dark" ? SUN_ICON : MOON_ICON}
          reducedMotion="user"
        />
      ) : (
        // The theme is unknown on the server, so CSS picks the icon until hydration.
        <>
          <Sun aria-hidden="true" className="hidden dark:block" />
          <Moon aria-hidden="true" className="dark:hidden" />
        </>
      )}
    </Button>
  );
}
