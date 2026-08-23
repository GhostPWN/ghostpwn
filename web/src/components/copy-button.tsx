"use client";

import * as React from "react";
import { Check, Copy } from "lucide-react";

export function CopyButton({ text }: { text: string }) {
  const [status, setStatus] = React.useState<"idle" | "copied" | "error">(
    "idle",
  );
  const resetTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );

  React.useEffect(
    () => () => {
      clearTimeout(resetTimer.current);
    },
    [],
  );

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setStatus("copied");
    } catch {
      setStatus("error");
    }

    clearTimeout(resetTimer.current);
    resetTimer.current = setTimeout(() => setStatus("idle"), 1500);
  };

  return (
    <>
      <span
        role="status"
        aria-live="polite"
        aria-atomic="true"
        className={
          status === "idle"
            ? "sr-only"
            : "absolute right-12 top-3 z-10 rounded-md border bg-background px-2 py-1 text-xs text-foreground shadow-sm"
        }
      >
        {status === "copied"
          ? "Copied"
          : status === "error"
            ? "Copy failed"
            : ""}
      </span>
      <button
        type="button"
        aria-label={status === "copied" ? "Code copied" : "Copy code"}
        onClick={copy}
        className="absolute right-3 top-3 z-10 inline-flex size-7 items-center justify-center rounded-md border bg-background/80 text-muted-foreground outline-none backdrop-blur transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background [&_svg]:size-3.5"
      >
        {status === "copied" ? (
          <Check aria-hidden="true" />
        ) : (
          <Copy aria-hidden="true" />
        )}
      </button>
    </>
  );
}
