import { codeToHtml } from "shiki";

interface CodeBlockProps {
  code: string;
  lang: string;
}

export async function CodeBlock({ code, lang }: CodeBlockProps) {
  const html = await codeToHtml(code, {
    lang,
    themes: { light: "github-light", dark: "github-dark" },
    defaultColor: false,
  });

  return (
    <div
      className="overflow-x-auto rounded-2xl border bg-card p-6 text-sm leading-relaxed [&_pre]:bg-transparent [&_pre]:font-mono [&_code]:font-mono"
      // shiki output is trusted, build-time generated from static strings
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
