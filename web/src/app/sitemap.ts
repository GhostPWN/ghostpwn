import type { MetadataRoute } from "next";
import { getDocSlugs } from "@/lib/docs-nav";

const SITE_URL = "https://ghostpwn.github.io/ghostpwn";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  const docs = getDocSlugs().map((slug) => ({
    url: `${SITE_URL}/docs/${slug.join("/")}${slug.length ? "/" : ""}`,
    changeFrequency: "monthly" as const,
    priority: slug.length ? 0.6 : 0.8,
  }));

  return [
    { url: `${SITE_URL}/`, changeFrequency: "monthly", priority: 1 },
    { url: `${SITE_URL}/privacy/`, changeFrequency: "yearly", priority: 0.3 },
    { url: `${SITE_URL}/cookies/`, changeFrequency: "yearly", priority: 0.3 },
    ...docs,
  ];
}
