import { tool } from "ai";
import { z } from "zod";
import { readdir, readFile, stat } from "node:fs/promises";
import { join, resolve } from "node:path";
import { Glob } from "bun";

export const agentTools = {
  readFile: tool({
    description:
      "Read the contents of a file. Use this to examine source code, configuration files, or any text file in the project.",
    inputSchema: z.object({
      path: z.string().describe("Absolute or relative file path to read"),
      maxLines: z
        .number()
        .optional()
        .describe("Maximum number of lines to return (default: all)"),
    }),
    execute: async ({ path, maxLines }) => {
      try {
        const resolved = resolve(path);
        const content = await readFile(resolved, "utf-8");
        const lines = content.split("\n");
        const truncated = maxLines ? lines.slice(0, maxLines) : lines;
        return {
          path: resolved,
          content: truncated.join("\n"),
          totalLines: lines.length,
          truncated: maxLines ? lines.length > maxLines : false,
        };
      } catch (err) {
        return {
          error: `Failed to read file: ${err instanceof Error ? err.message : String(err)}`,
        };
      }
    },
  }),

  listDirectory: tool({
    description:
      "List files and directories in a given path. Use this to explore the project structure.",
    inputSchema: z.object({
      path: z.string().describe("Directory path to list"),
    }),
    execute: async ({ path }) => {
      try {
        const resolved = resolve(path);
        const entries = await readdir(resolved, { withFileTypes: true });
        const items = entries.map((e) => ({
          name: e.name,
          type: e.isDirectory() ? "directory" : "file",
        }));
        return { path: resolved, entries: items };
      } catch (err) {
        return {
          error: `Failed to list directory: ${err instanceof Error ? err.message : String(err)}`,
        };
      }
    },
  }),

  searchFiles: tool({
    description:
      "Search for files matching a glob pattern. Use this to find specific file types or names across the project.",
    inputSchema: z.object({
      pattern: z
        .string()
        .describe('Glob pattern (e.g. "**/*.ts", "src/**/*.tsx")'),
      cwd: z
        .string()
        .optional()
        .describe("Base directory for the search (default: current directory)"),
    }),
    execute: async ({ pattern, cwd }) => {
      try {
        const base = resolve(cwd || ".");
        const glob = new Glob(pattern);
        const matches: string[] = [];
        for await (const match of glob.scan({
          cwd: base,
          dot: false,
        })) {
          matches.push(match);
          if (matches.length >= 100) break;
        }
        return { pattern, cwd: base, matches, truncated: matches.length >= 100 };
      } catch (err) {
        return {
          error: `Search failed: ${err instanceof Error ? err.message : String(err)}`,
        };
      }
    },
  }),

  grep: tool({
    description:
      "Search file contents for a regex pattern. Use this to find specific code, function definitions, imports, or text across files.",
    inputSchema: z.object({
      pattern: z.string().describe("Regex pattern to search for"),
      path: z
        .string()
        .optional()
        .describe("File or directory to search in (default: current directory)"),
      glob: z
        .string()
        .optional()
        .describe('File glob filter (e.g. "*.ts", "*.tsx")'),
    }),
    execute: async ({ pattern, path, glob: fileGlob }) => {
      try {
        const args = ["rg", "--json", "-m", "50", pattern];
        if (fileGlob) args.push("--glob", fileGlob);
        args.push(resolve(path || "."));

        const proc = Bun.spawn(args, {
          stdout: "pipe",
          stderr: "pipe",
        });
        const output = await new Response(proc.stdout).text();
        await proc.exited;

        const results: Array<{
          file: string;
          line: number;
          text: string;
        }> = [];

        for (const line of output.split("\n")) {
          if (!line) continue;
          try {
            const parsed = JSON.parse(line);
            if (parsed.type === "match") {
              results.push({
                file: parsed.data.path.text,
                line: parsed.data.line_number,
                text: parsed.data.lines.text.trimEnd(),
              });
            }
          } catch {
            // skip non-JSON lines
          }
        }

        return { pattern, matches: results, totalMatches: results.length };
      } catch (err) {
        return {
          error: `Grep failed: ${err instanceof Error ? err.message : String(err)}`,
        };
      }
    },
  }),

  runCommand: tool({
    description:
      "Execute a shell command and return its output. Use for running security tools, build commands, or any CLI operation. Commands run in the project root directory.",
    inputSchema: z.object({
      command: z.string().describe("Shell command to execute"),
      cwd: z
        .string()
        .optional()
        .describe("Working directory (default: project root)"),
      timeout: z
        .number()
        .optional()
        .describe("Timeout in milliseconds (default: 30000)"),
    }),
    execute: async ({ command, cwd, timeout }) => {
      try {
        const proc = Bun.spawn(["sh", "-c", command], {
          cwd: resolve(cwd || "."),
          stdout: "pipe",
          stderr: "pipe",
          env: process.env,
        });

        const timeoutMs = timeout || 30000;
        const timer = setTimeout(() => proc.kill(), timeoutMs);

        const [stdout, stderr] = await Promise.all([
          new Response(proc.stdout).text(),
          new Response(proc.stderr).text(),
        ]);

        clearTimeout(timer);
        const exitCode = await proc.exited;

        return {
          stdout: stdout.slice(0, 10000),
          stderr: stderr.slice(0, 5000),
          exitCode,
          truncated: stdout.length > 10000 || stderr.length > 5000,
        };
      } catch (err) {
        return {
          error: `Command failed: ${err instanceof Error ? err.message : String(err)}`,
        };
      }
    },
  }),

  fileInfo: tool({
    description:
      "Get metadata about a file or directory (size, type, modified time).",
    inputSchema: z.object({
      path: z.string().describe("Path to check"),
    }),
    execute: async ({ path }) => {
      try {
        const resolved = resolve(path);
        const stats = await stat(resolved);
        return {
          path: resolved,
          exists: true,
          type: stats.isDirectory() ? "directory" : "file",
          size: stats.size,
          modified: stats.mtime.toISOString(),
        };
      } catch {
        return { path: resolve(path), exists: false };
      }
    },
  }),
};
