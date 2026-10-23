import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";

import { expect, test } from "vitest";

const require = createRequire(import.meta.url);
const binaryName = process.platform === "win32" ? "tomlsmith.exe" : "tomlsmith";
const binaryPath = require.resolve(
  `@tomlsmith/cli-${process.platform}-${process.arch}/bin/${binaryName}`,
);

function runCli(args: string[], source: string) {
  const result = spawnSync(binaryPath, args, {
    input: source,
    encoding: "utf8",
    timeout: 10_000,
  });
  expect(result.error).toBeUndefined();
  return result;
}

test.each([
  {
    name: "the first LF in a mixed-newline document",
    source: "first=1\nsecond=2\r\n",
    expected: "first = 1\nsecond = 2\n",
  },
  {
    name: "the first CRLF in a mixed-newline document",
    source: "first=1\r\nsecond=2\n",
    expected: "first = 1\r\nsecond = 2\r\n",
  },
  {
    name: "an LF before a CRLF inside a multiline string",
    source: 'first=1\nmessage="""inside\r\nstring"""\n',
    expected: 'first = 1\nmessage = """inside\r\nstring"""\n',
  },
  {
    name: "the first physical newline inside a multiline string",
    source: 'message="""inside\nstring"""\r\nsecond=2\r\n',
    expected: 'message = """inside\nstring"""\nsecond = 2\n',
  },
  {
    name: "an absent final newline",
    source: "first=1",
    expected: "first = 1",
  },
])("preserve line endings follows $name", ({ source, expected }) => {
  const formatted = runCli(["fmt", "--line-ending", "preserve", "-"], source);
  expect(formatted.status, formatted.stderr).toBe(0);
  expect(formatted.stderr).toBe("");
  expect(formatted.stdout).toBe(expected);

  const secondPass = runCli(["fmt", "--line-ending", "preserve", "-"], formatted.stdout);
  expect(secondPass.status, secondPass.stderr).toBe(0);
  expect(secondPass.stdout).toBe(expected);

  const checked = runCli(["check", "-"], formatted.stdout);
  expect(checked.status, checked.stderr).toBe(0);
  expect(checked.stderr).toBe("");
});

function repeatedPackages(count: number): string {
  return Array.from({ length: count }, (_, index) =>
    `[[package]]\nname = "package-${index}"\n` +
    "[package.metadata.details]\nvalue = 1\n" +
    "[package.metadata]\nenabled = true\n" +
    "[[package.dependencies]]\nname = \"dep\"\n",
  ).join("");
}

test("repeated array-table scopes keep implicit promotion and duplicates local", () => {
  const source = repeatedPackages(32);
  const valid = runCli(["parse", "-"], source);
  expect(valid.status, valid.stderr).toBe(0);
  expect(JSON.parse(valid.stdout)).toMatchObject({ valid: true, diagnostics: [] });

  const duplicate = runCli(["parse", "-"], `${source}name = "duplicate"\n`);
  expect(duplicate.status).toBe(1);
  expect(JSON.parse(duplicate.stdout)).toMatchObject({
    valid: false,
    diagnostics: [
      {
        code: "semantic.duplicate-key",
        range: { start: source.length, end: source.length + 'name = "duplicate"'.length },
      },
    ],
  });
});

test("repeated array-table scopes retain explicit and implicit namespace conflicts", () => {
  const prefix = repeatedPackages(32);
  for (const tail of [
    "[[package]]\n[package.metadata.details]\nvalue = 1\n[[package.metadata]]\n",
    "[[package]]\n[package.metadata.details]\nvalue = 1\n[package.metadata]\ndetails.extra = 2\n",
  ]) {
    const result = runCli(["parse", "-"], prefix + tail);
    expect(result.status).toBe(1);
    expect(JSON.parse(result.stdout).diagnostics).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ code: "semantic.conflicting-key" }),
      ]),
    );
  }
});

test("formatting falls back to LF when wrapping a source with no newline", () => {
  const result = runCli(["fmt", "--line-width", "12", "-"], "root={value=1}");
  expect(result.status, result.stderr).toBe(0);
  expect(result.stdout).toBe("root = {\n  value = 1\n}");
});

test("promoting the current implicit table preserves earlier array-table scopes", () => {
  const source = "[[package]]\n[package.metadata.details]\nvalue = 1\n".repeat(32) +
    "[package.metadata]\nenabled = true\n";
  const result = runCli(["parse", "-"], source);
  expect(result.status, result.stderr).toBe(0);
  expect(JSON.parse(result.stdout)).toMatchObject({ valid: true, diagnostics: [] });

  const duplicate = runCli(["parse", "-"], `${source}[package.metadata]\n`);
  expect(duplicate.status).toBe(1);
  expect(JSON.parse(duplicate.stdout).diagnostics).toEqual([
    expect.objectContaining({ code: "semantic.conflicting-key" }),
  ]);
});
