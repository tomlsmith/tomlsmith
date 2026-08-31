import { copyFile, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { afterEach, expect, test } from "vitest";

const cliPath = fileURLToPath(new URL("../dist/cli.js", import.meta.url));
const repositoryRoot = fileURLToPath(new URL("../../..", import.meta.url));
const temporaryDirectories: string[] = [];

function platformArchiveLabel(): string {
  const labels: Readonly<Record<string, string | undefined>> = {
    "darwin-arm64": "darwin-arm64",
    "darwin-x64": "darwin-x64",
    "linux-arm64": "linux-arm64",
    "linux-x64": "linux-x64",
    "win32-x64": "win32-x64",
  };
  const label = labels[`${process.platform}-${process.arch}`];
  if (label === undefined) {
    throw new Error(
      `the npm e2e test has no package for ${process.platform}-${process.arch}`,
    );
  }
  return label;
}

afterEach(async () => {
  await Promise.all(
    temporaryDirectories.splice(0).map((directory) =>
      rm(directory, { recursive: true, force: true }),
    ),
  );
});

test("the npm executable delegates arguments, standard input, output, and status to TomlSmith", () => {
  const result = spawnSync(
    process.execPath,
    [cliPath, "parse", "--toml-version", "1.0", "-"],
    {
      encoding: "utf8",
      input: 'name = "TomlSmith"\n',
    },
  );

  expect(result.error).toBeUndefined();
  expect(result.status).toBe(0);
  expect(result.stderr).toBe("");
  expect(JSON.parse(result.stdout)).toEqual({
    diagnostics: [],
    tomlVersion: "1.0",
    valid: true,
  });

  const invalid = spawnSync(process.execPath, [cliPath, "check", "-"], {
    encoding: "utf8",
    input: "broken\n",
  });
  expect(invalid.status).toBe(1);
  expect(invalid.stdout).toBe("");
  expect(invalid.stderr).toContain("parse.missing-equals");
});

test("formatting keeps padding inside non-empty inline tables", () => {
  const source =
    'dependency={ version = "4\\u002e5", features=["derive"] } # keep\n' +
    "metadata={ nested={ enabled=true } }\n" +
    "empty={ }\n" +
    "multiline={\n value=1,\n}\n";
  const expected =
    'dependency = { version = "4\\u002e5", features = ["derive"] } # keep\n' +
    "metadata = { nested = { enabled = true } }\n" +
    "empty = {}\n" +
    "multiline = {\n  value = 1,\n}\n";

  const firstPass = spawnSync(process.execPath, [cliPath, "fmt", "-"], {
    encoding: "utf8",
    input: source,
  });

  expect(firstPass.error).toBeUndefined();
  expect(firstPass.status).toBe(0);
  expect(firstPass.stderr).toBe("");
  expect(firstPass.stdout).toBe(expected);

  const secondPass = spawnSync(process.execPath, [cliPath, "fmt", "-"], {
    encoding: "utf8",
    input: firstPass.stdout,
  });

  expect(secondPass.error).toBeUndefined();
  expect(secondPass.status).toBe(0);
  expect(secondPass.stderr).toBe("");
  expect(secondPass.stdout).toBe(expected);
});

test("the npm executable reports an omitted platform package without a stack trace", async () => {
  const directory = await mkdtemp(join(tmpdir(), "tomlsmith-npm-cli-"));
  temporaryDirectories.push(directory);
  const isolatedCli = join(directory, "cli.js");
  await copyFile(cliPath, isolatedCli);
  const environment = { ...process.env };
  delete environment.NODE_OPTIONS;
  delete environment.NODE_PATH;

  const result = spawnSync(process.execPath, [isolatedCli, "--version"], {
    encoding: "utf8",
    env: environment,
  });

  expect(result.status).toBe(2);
  expect(result.stdout).toBe("");
  expect(result.stderr).toContain("required native platform package is missing");
  expect(result.stderr).toContain("--omit=optional");
  expect(result.stderr).not.toContain(" at ");
});

test("the packed npm distribution installs an executable native CLI", async () => {
  const directory = await mkdtemp(join(tmpdir(), "tomlsmith-npm-install-"));
  temporaryDirectories.push(directory);
  const manifest = JSON.parse(
    await readFile(join(repositoryRoot, "npm", "cli", "package.json"), "utf8"),
  ) as { version: string };
  const archives = join(repositoryRoot, "npm", "dist");
  const cliArchive = join(
    archives,
    `tomlsmith-cli-${manifest.version}.tgz`,
  );
  const platformArchive = join(
    archives,
    `tomlsmith-cli-${platformArchiveLabel()}-${manifest.version}.tgz`,
  );
  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  const install = spawnSync(
    npm,
    [
      "install",
      "--prefix",
      directory,
      "--ignore-scripts",
      "--fetch-retries=0",
      "--registry=http://127.0.0.1:9",
      cliArchive,
      platformArchive,
    ],
    { encoding: "utf8", shell: process.platform === "win32" },
  );

  expect(install.status, install.stderr).toBe(0);
  const result = spawnSync(
    npm,
    ["exec", "--prefix", directory, "--", "tomlsmith", "--version"],
    { encoding: "utf8", shell: process.platform === "win32" },
  );

  expect(result.status, result.stderr).toBe(0);
  expect(result.stderr).toBe("");
  expect(result.stdout.trim()).toBe(`tomlsmith ${manifest.version}`);
});
