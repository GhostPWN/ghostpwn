// next/image with `unoptimized` does not prefix basePath onto public assets,
// so build the path explicitly. NEXT_PUBLIC_* is inlined at build time.
export const BASE_PATH = process.env.NEXT_PUBLIC_BASE_PATH ?? "";

export function asset(path: string): string {
  return `${BASE_PATH}${path}`;
}
