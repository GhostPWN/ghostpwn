import type { Metadata } from "next";
import { Geist_Mono, Inter } from "next/font/google";
import "./globals.css";
import { cn } from "@/lib/utils";
import { ThemeProvider } from "@/components/theme-provider";

const inter = Inter({ subsets: ["latin"], variable: "--font-sans" });

const geistMono = Geist_Mono({
  variable: "--font-mono",
  subsets: ["latin"],
});

const SITE_URL = "https://ghostpwn.github.io/ghostpwn";
const TITLE = "GhostPWN | Autonomous penetration testing agent";
const DESCRIPTION =
  "GhostPWN is a Rust terminal assistant for offensive security research, streaming from multiple LLM providers and running local tools inside a workspace boundary.";

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: TITLE,
    template: "%s | GhostPWN",
  },
  description: DESCRIPTION,
  alternates: { canonical: SITE_URL },
  openGraph: {
    type: "website",
    url: SITE_URL,
    siteName: "GhostPWN",
    title: TITLE,
    description: DESCRIPTION,
  },
  twitter: {
    card: "summary",
    title: TITLE,
    description: DESCRIPTION,
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={cn(
        "h-full antialiased font-sans",
        inter.variable,
        geistMono.variable,
      )}
    >
      <body className="min-h-full">
        <a
          href="#main-content"
          className="fixed left-[max(1rem,env(safe-area-inset-left))] top-[max(1rem,env(safe-area-inset-top))] z-50 -translate-y-20 rounded-md bg-background px-3 py-2 text-sm font-medium shadow-sm ring-2 ring-ring transition-transform focus:translate-y-0 motion-reduce:transition-none"
        >
          Skip to content
        </a>
        <ThemeProvider attribute="class" defaultTheme="dark" enableSystem>
          {children}
        </ThemeProvider>
      </body>
    </html>
  );
}
