import type { TocItem } from "@/lib/docs";

export function DocsToc({ items }: { items: TocItem[] }) {
  return (
    <nav aria-label="On this page" className="text-sm">
      <ul className="flex flex-col gap-1.5">
        {items.map((item) => (
          <li key={item.id} className={item.depth === 3 ? "pl-3" : undefined}>
            <a
              href={`#${item.id}`}
              className="block rounded-sm text-pretty text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            >
              {item.title}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
