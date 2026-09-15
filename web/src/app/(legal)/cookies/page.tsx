import type { Metadata } from "next";
import Link from "next/link";
import { LEGAL_UPDATED } from "../updated";

export const metadata: Metadata = {
  title: "Cookies policy",
  description:
    "How GhostPWN handles cookies, browser storage, and preferences.",
  alternates: { canonical: "https://ghostpwn.github.io/ghostpwn/cookies/" },
  openGraph: {
    title: "Cookies policy | GhostPWN",
    description:
      "How GhostPWN handles cookies, browser storage, and preferences.",
    type: "website",
  },
  twitter: {
    card: "summary",
    title: "Cookies policy | GhostPWN",
    description:
      "How GhostPWN handles cookies, browser storage, and preferences.",
  },
};

export default function PolicyPage() {
  return (
    <>
      <h1>Cookies policy</h1>
      <p className="text-xs text-muted-foreground tabular-nums">
        Last updated <time dateTime={LEGAL_UPDATED}>15 September 2026</time>
      </p>
      <h2>Cookies</h2>
      <p>
        The application does not set cookies. It has no advertising, analytics,
        or consent-management cookies. Its own browser storage supports
        preferences, not tracking.
      </p>
      <h2>What is stored instead</h2>
      <ul>
        <li>
          <code>theme</code>: the selected light, dark, or system theme. Stored
          in local storage until you clear site data; there is no automatic
          expiry.
        </li>
      </ul>
      <p>
        These values and caches have no fixed expiry. They remain until you
        clear them, the browser removes them, or the app replaces them. Local
        storage is not automatically attached to HTTP requests, and this app
        does not send these preferences to the server.
      </p>
      <p>
        Browser storage is shared by pages on the same origin, not isolated by
        URL path. Clearing site data can also reset preferences for other
        projects hosted on that origin.
      </p>
      <h2>Removing stored data</h2>
      <p>
        Open your browser’s site settings for this domain and clear site data.
        This removes local preferences. Browser-managed caches may also be
        cleared through browsing-data settings. Downloaded files and clipboard
        contents are separate and must be removed using your device’s controls.
      </p>
      <h2>Why there is no consent banner</h2>
      <p>
        There are no optional tracking or advertising technologies to accept.
        Storage supports the site’s functions and preferences. If optional
        tracking is introduced, this policy will change and consent will be
        requested before it starts where required.
      </p>
      <h2>Third parties and server logs</h2>
      <p>
        Links to external sites are governed by their policies once you follow
        them. Server request logs are separate from browser storage and are
        covered by the <Link href="/privacy">privacy policy</Link>.
      </p>
      <h2>Questions and changes</h2>
      <p>
        Contact{" "}
        <a href="mailto:contact@gaya.anonaddy.com">contact@gaya.anonaddy.com</a>
        . Changes to storage will be described here with a revised update date.
      </p>
      <p>
        <Link href="/privacy">Privacy policy</Link> ·{" "}
        <Link href="/">Back to GhostPWN</Link>
      </p>
    </>
  );
}
