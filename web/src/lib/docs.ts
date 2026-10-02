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

interface HastNode {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: HastNode[];
}

export interface TocItem {
  id: string;
  title: string;
  depth: 2 | 3;
}

declare module "vfile" {
  interface DataMap {
    toc: TocItem[];
  }
}

function textOf(node: HastNode): string {
  if (node.type === "text") return node.value ?? "";
  return (node.children ?? []).map(textOf).join("");
}

function containsLink(node: HastNode): boolean {
  return (node.children ?? []).some(
    (child) => child.tagName === "a" || containsLink(child),
  );
}

/**
 * Collect h2/h3 for the "On this page" outline and turn each heading into a
 * link to itself. Runs after rehype-slug so every heading already has an id.
 */
function rehypeHeadingLinks() {
  return (tree: HastNode, file: { data: { toc?: TocItem[] } }) => {
    const toc: TocItem[] = [];
    const walk = (node: HastNode) => {
      for (const child of node.children ?? []) {
        const depth = child.tagName === "h2" ? 2 : child.tagName === "h3" ? 3 : 0;
        const id = child.properties?.id;
        if (child.type !== "element" || !depth || typeof id !== "string") {
          walk(child);
          continue;
        }
        toc.push({ id, title: textOf(child), depth });
        // A heading that already holds a link cannot wrap another one.
        if (!containsLink(child)) {
          child.children = [
            {
              type: "element",
              tagName: "a",
              properties: { href: `#${id}`, className: ["heading-anchor"] },
              children: child.children ?? [],
            },
          ];
        }
      }
    };
    walk(tree);
    file.data.toc = toc;
  };
}

const processor = unified()
  .use(remarkParse)
  .use(remarkGfm)
  .use(remarkRehype)
  .use(rehypeSlug)
  .use(rehypeHeadingLinks)
  .use(rehypePrettyCode, {
    theme: { light: "github-light", dark: "github-dark" },
    keepBackground: false,
  })
  .use(rehypeStringify);

export interface RenderedDoc {
  html: string;
  title: string;
  toc: TocItem[];
  description?: string;
}

function parseFrontmatter(raw: string): { body: string; frontmatter: Record<string, string> } {
  if (!raw.startsWith("---\n")) {
    return { body: raw, frontmatter: {} };
  }

  const end = raw.indexOf("\n---", 4);
  if (end === -1) {
    return { body: raw, frontmatter: {} };
  }

  const block = raw.slice(4, end).trim();
  const body = raw.slice(end + "\n---".length).replace(/^\n/, "");
  const frontmatter = Object.fromEntries(
    block
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const separator = line.indexOf(":");
        if (separator === -1) return [line, ""];
        const key = line.slice(0, separator).trim();
        const value = line.slice(separator + 1).trim().replace(/^['\"]|['\"]$/g, "");
        return [key, value];
      }),
  );

  return { body, frontmatter };
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

  const { body, frontmatter } = parseFrontmatter(raw);
  const title = frontmatter.title ?? body.match(/^#\s+(.+)$/m)?.[1]?.trim() ?? "GhostPWN Docs";
  const file = await processor.process(body);
  let html = String(file);
  const basePath = process.env.NEXT_PUBLIC_BASE_PATH || "";
  if (basePath) {
    html = html.replaceAll('href="/', `href="${basePath}/`);
  }
  return { html, title, toc: file.data.toc ?? [], description: frontmatter.description };
}
