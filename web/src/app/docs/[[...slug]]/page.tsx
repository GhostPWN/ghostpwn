import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { getDoc, getDocSlugs } from "@/lib/docs";
import { CodeCopy } from "@/components/code-copy";

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
  return { title: doc ? `${doc.title} — WebSkrap` : "WebSkrap Docs" };
}

export default async function DocPage(props: DocPageProps) {
  const { slug = [] } = await props.params;
  const doc = await getDoc(slug);
  if (!doc) notFound();

  return (
    <>
      <article
        className="prose prose-neutral max-w-none dark:prose-invert prose-headings:scroll-mt-24 prose-pre:rounded-2xl prose-pre:border prose-pre:bg-card prose-pre:p-6 prose-a:text-primary"
        dangerouslySetInnerHTML={{ __html: doc.html }}
      />
      <CodeCopy />
    </>
  );
}
