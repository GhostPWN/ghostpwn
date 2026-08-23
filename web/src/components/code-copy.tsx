"use client";

import { useEffect } from "react";

const COPY_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>`;
const CHECK_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>`;
const STATUS_CLASS =
  "absolute right-12 top-3 z-10 rounded-md border bg-background px-2 py-1 text-xs text-foreground shadow-sm";

// Docs HTML is server-rendered (dangerouslySetInnerHTML), so attach copy
// buttons to each <pre> on the client after mount.
export function CodeCopy() {
  useEffect(() => {
    const main = document.querySelector("#main-content");
    if (!main) return;

    const enhanceCodeBlocks = () => {
      const pres = main.querySelectorAll<HTMLPreElement>("article pre");
      pres.forEach((pre) => {
        if (pre.dataset.copyReady) return;
        pre.dataset.copyReady = "1";
        const text = pre.innerText;

        const wrapper = document.createElement("div");
        wrapper.className = "relative";
        pre.replaceWith(wrapper);
        wrapper.appendChild(pre);

        const btn = document.createElement("button");
        btn.type = "button";
        btn.setAttribute("aria-label", "Copy code");
        btn.innerHTML = COPY_SVG;
        btn.className =
          "absolute right-3 top-3 z-10 inline-flex size-7 items-center justify-center rounded-md border bg-background/80 text-muted-foreground outline-none backdrop-blur transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background";

        const status = document.createElement("span");
        status.setAttribute("role", "status");
        status.setAttribute("aria-live", "polite");
        status.setAttribute("aria-atomic", "true");
        status.className = "sr-only";

        let resetTimer: ReturnType<typeof setTimeout> | undefined;
        btn.addEventListener("click", async () => {
          try {
            await navigator.clipboard.writeText(text);
            btn.innerHTML = CHECK_SVG;
            btn.setAttribute("aria-label", "Code copied");
            status.className = STATUS_CLASS;
            status.textContent = "Copied";
          } catch {
            status.className = STATUS_CLASS;
            status.textContent = "Copy failed";
          }

          clearTimeout(resetTimer);
          resetTimer = setTimeout(() => {
            btn.innerHTML = COPY_SVG;
            btn.setAttribute("aria-label", "Copy code");
            status.textContent = "";
            status.className = "sr-only";
          }, 1500);
        });
        wrapper.appendChild(status);
        wrapper.appendChild(btn);
      });
    };

    enhanceCodeBlocks();
    const observer = new MutationObserver(enhanceCodeBlocks);
    observer.observe(main, { childList: true, subtree: true });

    return () => observer.disconnect();
  }, []);

  return null;
}
