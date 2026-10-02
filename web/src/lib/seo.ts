export const SITE_URL = "https://ghostpwn.github.io/ghostpwn";
export const SITE_NAME = "GhostPWN";
export const SOCIAL_IMAGE_URL = `${SITE_URL}/ghostpwn-og.png`;
export const DEFAULT_TITLE = "GhostPWN | Autonomous penetration testing agent";
export const DEFAULT_DESCRIPTION =
  "GhostPWN is a Rust terminal assistant for offensive security research, streaming from multiple LLM providers and running local tools inside a workspace boundary.";

export function canonicalUrl(path = "/"): string {
  const normalized = path.startsWith("/") ? path : `/${path}`;
  return `${SITE_URL}${normalized}`;
}
