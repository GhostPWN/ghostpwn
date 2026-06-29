import { promises as fs } from "node:fs";
import path from "node:path";
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkRehype from "remark-rehype";
import rehypeSlug from "rehype-slug";
import rehypePrettyCode from "rehype-pretty-code";
import rehypeStringify from "rehype-stringify";

export { NAV, getDocSlugs } from "@/lib/docs-nav";
export type { NavItem, NavSection } from "@/lib/docs-nav";

const CONTENT_DIR = path.join(process.cwd(), "content");

const processor = unified()
  .use(remarkParse)
  .use(remarkGfm)
  .use(remarkRehype)
  .use(rehypeSlug)
  .use(rehypePrettyCode, {
    theme: { light: "github-light", dark: "github-dark" },
    keepBackground: false,
  })
  .use(rehypeStringify);

export interface RenderedDoc {
  html: string;
  title: string;
}

export async function getDoc(slug: string[]): Promise<RenderedDoc | null> {
  const relative = slug.length ? path.join(...slug) : "index";
  const filePath = path.join(CONTENT_DIR, `${relative}.md`);
  if (!filePath.startsWith(CONTENT_DIR)) {
    return null;
  }

  let raw: string;
  try {
    raw = await fs.readFile(filePath, "utf8");
  } catch {
    return null;
  }

  const title = raw.match(/^#\s+(.+)$/m)?.[1]?.trim() ?? "WebSkrap Docs";
  let html = String(await processor.process(raw));
  const basePath = process.env.NEXT_PUBLIC_BASE_PATH || "";
  if (basePath) {
    html = html.replaceAll('href="/', `href="${basePath}/`);
  }
  return { html, title };
}
