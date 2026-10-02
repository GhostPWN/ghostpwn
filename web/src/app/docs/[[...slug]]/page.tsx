import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { getDoc, getDocSlugs } from "@/lib/docs";
import {
  canonicalUrl,
  DEFAULT_DESCRIPTION,
  SITE_NAME,
  SOCIAL_IMAGE_URL,
} from "@/lib/seo";
import { CodeCopy } from "@/components/code-copy";
import { DocsPager } from "@/components/docs-pager";
import { DocsToc } from "@/components/docs-toc";

export const dynamicParams = false;

export function generateStaticParams() {
  return getDocSlugs().map((slug) => ({ slug }));
}

interface DocPageProps {
  params: Promise<{ slug?: string[] }>;
}

export async function generateMetadata(props: DocPageProps): Promise<Metadata> {
  const { slug = [] } = await props.params;
  const doc = await getDoc(slug);
  const path = slug.length ? `/docs/${slug.join("/")}/` : "/docs/";
  const title = doc && slug.length ? doc.title : "Documentation";
  const description = doc?.description ?? DEFAULT_DESCRIPTION;

  return {
    title,
    description,
    alternates: { canonical: canonicalUrl(path) },
    openGraph: {
      type: "article",
      url: canonicalUrl(path),
      title: `${title} | ${SITE_NAME}`,
      description,
      siteName: SITE_NAME,
      images: [{ url: SOCIAL_IMAGE_URL, width: 1200, height: 630, alt: `${SITE_NAME} documentation` }],
    },
    twitter: {
      card: "summary_large_image",
      title: `${title} | ${SITE_NAME}`,
      description,
      images: [SOCIAL_IMAGE_URL],
    },
  };
}

export default async function DocPage(props: DocPageProps) {
  const { slug = [] } = await props.params;
  const doc = await getDoc(slug);
  if (!doc) notFound();
  const href = slug.length ? `/docs/${slug.join("/")}` : "/docs";

  return (
    <>
      <div className="xl:grid xl:grid-cols-[minmax(0,1fr)_12rem] xl:gap-10">
        <div className="min-w-0">
          {doc.toc.length > 1 && (
            <details className="mb-8 rounded-lg border px-4 py-3 text-sm xl:hidden">
              <summary className="cursor-pointer font-medium">On this page</summary>
              <div className="mt-3">
                <DocsToc items={doc.toc} />
              </div>
            </details>
          )}
          <article
            className="prose prose-neutral max-w-none dark:prose-invert prose-headings:scroll-mt-24 prose-headings:text-balance prose-p:text-pretty prose-pre:rounded-lg prose-pre:border prose-pre:bg-card prose-pre:p-6 prose-a:text-brand"
            dangerouslySetInnerHTML={{ __html: doc.html }}
          />
          <DocsPager href={href} />
        </div>
        {doc.toc.length > 1 && (
          <aside className="hidden xl:block">
            <div className="sticky top-24 flex max-h-[calc(100dvh-7rem)] flex-col gap-3 overflow-y-auto">
              <p className="text-xs font-semibold uppercase text-muted-foreground">
                On this page
              </p>
              <DocsToc items={doc.toc} />
            </div>
          </aside>
        )}
      </div>
      <CodeCopy />
    </>
  );
}
