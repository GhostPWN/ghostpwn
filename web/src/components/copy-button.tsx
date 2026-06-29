"use client";

import * as React from "react";
import { Check, Copy } from "lucide-react";

export function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = React.useState(false);

  return (
    <button
      type="button"
      aria-label="Copy code"
      onClick={async () => {
        await navigator.clipboard.writeText(text);
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      }}
      className="absolute right-3 top-3 z-10 inline-flex size-7 items-center justify-center rounded-md border bg-background/80 text-muted-foreground backdrop-blur transition-colors hover:text-foreground [&_svg]:size-3.5"
    >
      {copied ? <Check /> : <Copy />}
    </button>
  );
}
