import { SiteHeader } from "@/components/site-header";
import { SiteFooter } from "@/components/site-footer";
import "./legal.css";

export default function LegalLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-h-dvh flex-col" lang="en">
      <SiteHeader />
      <main
        id="main-content"
        tabIndex={-1}
        className="prose-legal mx-auto w-full max-w-3xl flex-1 px-6 py-12 text-sm text-foreground sm:py-16"
      >
        {children}
      </main>
      <SiteFooter />
    </div>
  );
}
