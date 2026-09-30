import type { Metadata } from "next";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Terms of use",
  description: "Terms for the GhostPWN website, documentation, and software.",
  alternates: { canonical: "https://ghostpwn.github.io/ghostpwn/terms/" },
  openGraph: {
    title: "Terms of use | GhostPWN",
    description: "Terms for the GhostPWN website, documentation, and software.",
    type: "website",
    url: "https://ghostpwn.github.io/ghostpwn/terms/",
  },
  twitter: {
    card: "summary",
    title: "Terms of use | GhostPWN",
    description: "Terms for the GhostPWN website, documentation, and software.",
  },
};

export default function TermsPage() {
  return (
    <>
      <h1>Terms of use</h1>
      <p className="text-xs text-muted-foreground tabular-nums">
        Last updated <time dateTime="2026-09-30">30 September 2026</time>
      </p>
      <h2>Scope</h2>
      <p>
        These terms cover this website and its documentation. The GhostPWN
        software is governed by its license, described below. Using the site
        means you accept these terms.
      </p>
      <h2>Software license</h2>
      <p>
        GhostPWN is open-source software released under the{" "}
        <a href="https://github.com/GhostPWN/ghostpwn/blob/main/LICENSE">
          MIT License
        </a>
        . The license alone sets your rights to use, modify, and redistribute
        the code, and nothing here narrows or extends it. The documentation and
        site source live in the same repository under the same license unless a
        file says otherwise.
      </p>
      <h2>Responsible use</h2>
      <p>
        GhostPWN can run commands, read files, and contact model providers.
        Use it only on systems you own or have explicit permission to test.
        You are responsible for the scope of each assessment, the commands you
        approve, and compliance with applicable laws and agreements. Review
        generated findings before acting on them and protect credentials and
        personal data when sharing context with a provider.
      </p>
      <h2>Documentation and results</h2>
      <p>
        Documentation and examples are provided for information and may lag
        the latest release. Results depend on your inputs and environment;
        review them before relying on them.
      </p>
      <h2>No warranty</h2>
      <p>
        The site, documentation, and software are provided “as is”, without
        warranties of any kind. To the extent the law allows, the operator is
        not liable for damages arising from their use, including loss of
        data or claims by third parties. Nothing here limits liability that cannot be limited by law, or rights you hold as a
        consumer.
      </p>
      <h2>Trademarks</h2>
      <p>
        Third-party names and marks belong to their owners. References to
        them describe compatibility or functionality. GhostPWN is not
        affiliated with or endorsed by those owners.
      </p>
      <h2>Governing law</h2>
      <p>
        These terms are governed by French law. If you are a consumer, you also
        keep the protection of the mandatory rules of your country of
        residence.
      </p>
      <h2>Changes and contact</h2>
      <p>
        These terms may change; the date above identifies the latest revision.
        Questions go to{" "}
        <a href="mailto:contact@gaya.anonaddy.com">contact@gaya.anonaddy.com</a>
        .
      </p>
      <p>
        <Link href="/legal-notice">Legal notice</Link> ·{" "}
        <Link href="/privacy">Privacy policy</Link> ·{" "}
        <Link href="/">Back to GhostPWN</Link>
      </p>
    </>
  );
}
