export interface NavItem {
  title: string;
  href: string;
}

export interface NavSection {
  title?: string;
  items: NavItem[];
}

export const NAV: NavSection[] = [
  { items: [{ title: "Home", href: "/docs" }] },
  {
    title: "Getting Started",
    items: [
      { title: "Installation", href: "/docs/getting-started/installation" },
      { title: "Commands", href: "/docs/getting-started/commands" },
      { title: "Image input", href: "/docs/getting-started/image-input" },
    ],
  },
  {
    title: "Reference",
    items: [
      { title: "Configuration", href: "/docs/configuration" },
      { title: "Architecture", href: "/docs/architecture" },
    ],
  },
];

// Every doc slug derived from the nav (["/docs"] -> []).
export function getDocSlugs(): string[][] {
  return NAV.flatMap((section) =>
    section.items.map((item) =>
      item.href.replace(/^\/docs\/?/, "").split("/").filter(Boolean),
    ),
  );
}
