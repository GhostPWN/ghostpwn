"use client";

import { useEffect } from "react";

const COPY_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>`;
const CHECK_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>`;

// Docs HTML is server-rendered (dangerouslySetInnerHTML), so attach copy
// buttons to each <pre> on the client after mount.
export function CodeCopy() {
  useEffect(() => {
    const pres = document.querySelectorAll<HTMLPreElement>("article pre");
    pres.forEach((pre) => {
      if (pre.dataset.copyReady) return;
      pre.dataset.copyReady = "1";
      pre.classList.add("relative");
      const text = pre.innerText;

      const btn = document.createElement("button");
      btn.type = "button";
      btn.setAttribute("aria-label", "Copy code");
      btn.innerHTML = COPY_SVG;
      btn.className =
        "absolute right-3 top-3 z-10 inline-flex size-7 items-center justify-center rounded-md border bg-background/80 text-muted-foreground backdrop-blur transition-colors hover:text-foreground";
      btn.addEventListener("click", async () => {
        await navigator.clipboard.writeText(text);
        btn.innerHTML = CHECK_SVG;
        setTimeout(() => {
          btn.innerHTML = COPY_SVG;
        }, 1500);
      });
      pre.appendChild(btn);
    });
  }, []);

  return null;
}
