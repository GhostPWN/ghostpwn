import { SiteFooter } from "@/components/site-footer";
import { SiteHeader } from "@/components/site-header";
import { DocsSidebar } from "@/components/docs-sidebar";

export default function DocsLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-h-full flex-col">
      <SiteHeader showDocsLink={false} mobileMenu={<DocsSidebar />} />

      <div className="mx-auto flex w-full max-w-6xl flex-1 gap-10 px-6">
        <aside className="hidden w-56 shrink-0 py-10 md:block">
          <div className="sticky top-24">
            <DocsSidebar />
          </div>
        </aside>
        <main id="main-content" tabIndex={-1} className="min-w-0 flex-1 py-10">
          {children}
        </main>
      </div>
      <SiteFooter />
    </div>
  );
}
