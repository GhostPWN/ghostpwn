import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

const require = createRequire(import.meta.url);
const { getRootDirs } = require("@next/eslint-plugin-next/dist/utils/get-root-dirs");
const { globSync } = require("fast-glob");

test("Next lint root discovery preserves directory glob behavior", () => {
  const directory = mkdtempSync(path.join(tmpdir(), "ghostpwn-lint-"));
  try {
    for (const name of ["app", "docs", "other"]) {
      mkdirSync(path.join(directory, name));
    }
    writeFileSync(path.join(directory, "file"), "");
    const discover = (rootDir) => getRootDirs({
      cwd: directory,
      settings: { next: { rootDir } },
    }).map((root) => path.resolve(root)).sort();

    assert.deepEqual(discover(undefined), [directory]);
    assert.deepEqual(discover(`${directory}/*`), ["app", "docs", "other"].map((name) => path.join(directory, name)));
    assert.deepEqual(discover(`${directory}/{app,docs}`), ["app", "docs"].map((name) => path.join(directory, name)));
    assert.deepEqual(discover([`${directory}/app`, `${directory}/docs`]), ["app", "docs"].map((name) => path.join(directory, name)));
    assert.deepEqual(discover(`${directory}/missing*`), []);
    assert.deepEqual(discover(`${directory}\\app`), [path.join(directory, "app")]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("deeply nested brace patterns do not overflow the stack", () => {
  const pattern = "{".repeat(4000) + "a,b" + "}".repeat(4000);
  assert.doesNotThrow(() => globSync(pattern, { onlyDirectories: true }));
});
