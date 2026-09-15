import type { Metadata } from "next";
import Link from "next/link";
import { LEGAL_UPDATED } from "../updated";

export const metadata: Metadata = {
  title: "Privacy policy",
  description: "How GhostPWN handles data, hosting requests, and your privacy.",
  alternates: { canonical: "https://ghostpwn.github.io/ghostpwn/privacy/" },
  openGraph: {
    title: "Privacy policy | GhostPWN",
    description:
      "How GhostPWN handles data, hosting requests, and your privacy.",
    type: "website",
  },
  twitter: {
    card: "summary",
    title: "Privacy policy | GhostPWN",
    description:
      "How GhostPWN handles data, hosting requests, and your privacy.",
  },
};

export default function PolicyPage() {
  return (
    <>
      <h1>Privacy policy</h1>
      <p className="text-xs text-muted-foreground tabular-nums">
        Last updated <time dateTime={LEGAL_UPDATED}>15 September 2026</time>
      </p>
      <h2>About this site</h2>
      <p>
        This policy covers the GhostPWN website and documentation. It does not
        cover running the software yourself or services you connect it to. The
        site has no accounts, advertising, audience analytics, or tracking
        scripts. We do not sell visitor data or use it for marketing.
      </p>
      <h2>Who is responsible</h2>
      <p>
        Gaya KACI operates this site. For privacy questions or requests, email{" "}
        <a href="mailto:contact@gaya.anonaddy.com">contact@gaya.anonaddy.com</a>
        .
      </p>
      <h2>Hosting and request logs</h2>
      <p>
        This site is published with GitHub Pages. Serving a page requires GitHub
        to process your IP address and request details under its own privacy
        statement.
      </p>
      <p>
        GitHub controls its hosting logs, retention, and infrastructure. I do
        not receive or control those logs. See GitHub’s <a href="https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement">privacy statement</a> for details.
      </p>
      <h2>Browser storage</h2>
      <p>
        The site remembers its theme in local storage. This is a browser
        preference, not a visitor identifier, and the app does not transmit it
        to its server. The <Link href="/cookies">cookies policy</Link> lists
        storage and explains how to clear it.
      </p>
      <h2>Contact and external links</h2>
      <p>
        If you email{" "}
        <a href="mailto:contact@gaya.anonaddy.com">contact@gaya.anonaddy.com</a>
        , addy.io forwards the message to the operator’s mailbox. Those mail
        services process the message to deliver it. Correspondence is kept while
        needed to answer your request; you can ask for deletion.
      </p>
      <p>
        External links take you to independently operated services under their
        own policies. Fonts and site assets are served with the site; there are
        no embedded advertising or analytics widgets.
      </p>
      <h2>Your rights</h2>
      <p>
        Where the GDPR applies, you can request access, correction, erasure,
        restriction, or portability where applicable, and object to processing
        based on legitimate interests. Operating and securing the site and
        answering correspondence rely on those legitimate interests. Email the
        operator to exercise these rights. We normally respond within one month.
        You can also complain to your data protection authority, including the{" "}
        <a href="https://www.cnil.fr/">CNIL</a> in France.
      </p>
      <p>
        Browser-only files and preferences are not available to the operator.
        For those, use your browser’s site-data controls or remove your
        downloaded files yourself.
      </p>
      <h2>Changes</h2>
      <p>
        This page will be updated when the site’s data handling changes. The
        date above identifies the latest revision.
      </p>
      <p>
        <Link href="/cookies">Cookies policy</Link> ·{" "}
        <Link href="/">Back to GhostPWN</Link>
      </p>
    </>
  );
}
