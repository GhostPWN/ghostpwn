import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Legal notice",
  description: "Publisher, hosting, and intellectual property information for GhostPWN.",
  alternates: { canonical: "https://ghostpwn.github.io/ghostpwn/legal-notice/" },
  openGraph: {
    title: "Legal notice | GhostPWN",
    description: "Publisher, hosting, and intellectual property information for GhostPWN.",
    type: "website",
    url: "https://ghostpwn.github.io/ghostpwn/legal-notice/",
  },
  twitter: {
    card: "summary",
    title: "Legal notice | GhostPWN",
    description: "Publisher, hosting, and intellectual property information for GhostPWN.",
  },
};

export default function LegalNoticePage() {
  return (
    <>
      <h1>Legal notice</h1>
      <p className="text-xs text-muted-foreground tabular-nums">
        Last updated <time dateTime="2026-09-30">30 September 2026</time>
      </p>
      <h2>Publisher</h2>
      <p>
        This site is published by Gaya KACI, an individual, on a non-commercial
        basis. Gaya KACI is also the publication director. Contact:{" "}
        <a href="mailto:contact@gaya.anonaddy.com">contact@gaya.anonaddy.com</a>
        .
      </p>
      <h2>Hosting</h2>
      <p>
        GitHub, Inc., 88 Colin P. Kelly Jr. Street, San Francisco, CA 94107,
        United States. <a href="https://github.com">github.com</a>
      </p>
      <h2>Intellectual property</h2>
      <p>
        The GhostPWN code, documentation, and site source are available under
        the{" "}
        <a href="https://github.com/GhostPWN/ghostpwn/blob/main/LICENSE">
          MIT License
        </a>
        . Third-party names and marks belong to their owners; see the{" "}
        <Link href="/terms">terms of use</Link>.
      </p>
      <h2>Security reports</h2>
      <p>
        Report vulnerabilities privately by email to{" "}
        <a href="mailto:contact@gaya.anonaddy.com">contact@gaya.anonaddy.com</a>
        . Do not open public issues for unpatched vulnerabilities.
      </p>
      <h2>Content concerns</h2>
      <p>
        To report unlawful content or an infringement on this site, email{" "}
        <a href="mailto:contact@gaya.anonaddy.com">contact@gaya.anonaddy.com</a>{" "}
        with the page address and the reason for your request.
      </p>
      <p>
        <Link href="/terms">Terms of use</Link> ·{" "}
        <Link href="/privacy">Privacy policy</Link> ·{" "}
        <Link href="/cookies">Cookies policy</Link> ·{" "}
        <Link href="/">Back to GhostPWN</Link>
      </p>
    </>
  );
}
