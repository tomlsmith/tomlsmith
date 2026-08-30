import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { afterEach, expect, test } from "vitest";

const repositoryRoot = fileURLToPath(new URL("../..", import.meta.url));
const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories.splice(0).map((directory) =>
      rm(directory, { recursive: true, force: true }),
    ),
  );
});

test("publish retries reject an existing package with different contents", async () => {
  const directory = await mkdtemp(join(tmpdir(), "tomlsmith-npm-publish-"));
  temporaryDirectories.push(directory);
  const archives = join(directory, "archives");
  const fakeBin = join(directory, "bin");
  const publishLog = join(directory, "published.log");
  await Promise.all([
    mkdir(archives, { recursive: true }),
    mkdir(fakeBin, { recursive: true }),
  ]);

  const manifest = JSON.parse(
    await readFile(join(repositoryRoot, "npm", "cli", "package.json"), "utf8"),
  ) as { version: string };
  for (const label of [
    "darwin-arm64",
    "darwin-x64",
    "linux-arm64",
    "linux-x64",
    "win32-x64",
  ]) {
    await writeFile(
      join(archives, `tomlsmith-cli-${label}-${manifest.version}.tgz`),
      `local archive for ${label}`,
    );
  }
  await writeFile(
    join(archives, `tomlsmith-cli-${manifest.version}.tgz`),
    "local public archive",
  );

  const fakeNpm = join(fakeBin, "fake-npm.mjs");
  await writeFile(
    fakeNpm,
    `import { appendFileSync } from "node:fs";
const args = process.argv.slice(2);
if (args[0] === "view" && args[2] === "version") {
  process.stdout.write(JSON.stringify(${JSON.stringify(manifest.version)}));
  process.exit(0);
}
if (args[0] === "view" && args[2] === "dist.integrity") {
  process.stdout.write(JSON.stringify("sha512-different-registry-archive"));
  process.exit(0);
}
if (args[0] === "publish") {
  appendFileSync(${JSON.stringify(publishLog)}, args.join(" ") + "\\n");
  process.exit(0);
}
process.stderr.write("unexpected fake npm invocation: " + args.join(" "));
process.exit(2);
`,
  );

  if (process.platform === "win32") {
    await writeFile(
      join(fakeBin, "npm.cmd"),
      `@echo off\r\n"${process.execPath}" "${fakeNpm}" %*\r\n`,
    );
  } else {
    const npm = join(fakeBin, "npm");
    await writeFile(npm, `#!/bin/sh\nexec "${process.execPath}" "${fakeNpm}" "$@"\n`);
    await chmod(npm, 0o755);
  }

  const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
  const result = spawnSync(
    pnpm,
    ["exec", "tsx", "npm/scripts/package.ts", "publish", "--input", archives],
    {
      cwd: repositoryRoot,
      encoding: "utf8",
      env: {
        ...process.env,
        PATH: `${fakeBin}${delimiter}${process.env.PATH ?? ""}`,
      },
      shell: process.platform === "win32",
    },
  );

  expect(result.status).not.toBe(0);
  expect(result.stderr).toContain("integrity");
  expect(await readFile(publishLog, "utf8").catch(() => "")).toBe("");
});
