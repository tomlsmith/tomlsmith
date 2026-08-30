import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";

import { Command } from "commander";

import {
  findNativePlatform,
  nativePlatforms,
  supportedPlatformLabels,
  type NativePlatform,
} from "../platforms";

const repositoryRoot = fileURLToPath(new URL("../..", import.meta.url));

interface StageOptions {
  readonly binary: string;
  readonly platform: string;
  readonly arch: string;
}

interface PackOptions {
  readonly output: string;
  readonly platform: string;
  readonly arch: string;
  readonly includeCli: boolean;
}

interface OutputOptions {
  readonly output: string;
}

interface PublishOptions {
  readonly input: string;
}

interface PackageManifest {
  readonly name?: string;
  readonly version?: string;
  readonly private?: boolean;
  readonly files?: readonly string[];
  readonly optionalDependencies?: Readonly<Record<string, string>>;
}

interface CargoMetadata {
  readonly packages: readonly {
    readonly name: string;
    readonly version: string;
    readonly publish: readonly string[] | null;
  }[];
}

async function regularFile(path: string): Promise<boolean> {
  return (await stat(path).catch(() => undefined))?.isFile() === true;
}

async function archiveIntegrity(path: string): Promise<string> {
  const archive = await readFile(path);
  return `sha512-${createHash("sha512").update(archive).digest("base64")}`;
}

async function resolveSourceBinary(
  binary: string,
  nativePlatform: NativePlatform,
): Promise<string> {
  const source = resolve(binary);
  if (await regularFile(source)) {
    return source;
  }
  if (
    nativePlatform.platform === "win32" &&
    !source.toLowerCase().endsWith(".exe") &&
    (await regularFile(`${source}.exe`))
  ) {
    return `${source}.exe`;
  }
  throw new Error(`native binary is not a regular file: ${source}`);
}

function platformPackageDirectory(nativePlatform: NativePlatform): string {
  return resolve(
    repositoryRoot,
    "npm",
    "platforms",
    nativePlatform.directory,
  );
}

function stagedBinaryPath(nativePlatform: NativePlatform): string {
  return resolve(
    platformPackageDirectory(nativePlatform),
    "bin",
    nativePlatform.binaryName,
  );
}

async function stageNativeBinary(options: StageOptions): Promise<void> {
  const nativePlatform = findNativePlatform(options.platform, options.arch);
  if (nativePlatform === undefined) {
    throw new Error(
      `unsupported platform ${options.platform}-${options.arch}; expected one of ${supportedPlatformLabels()}`,
    );
  }

  const source = await resolveSourceBinary(options.binary, nativePlatform);
  const destination = stagedBinaryPath(nativePlatform);
  await mkdir(dirname(destination), { recursive: true });
  await copyFile(source, destination);
  if (nativePlatform.platform !== "win32") {
    await chmod(destination, 0o755);
  }
  process.stdout.write(`staged ${nativePlatform.packageName}: ${destination}\n`);
}

async function readPackageManifest(path: string): Promise<PackageManifest> {
  return JSON.parse(await readFile(path, "utf8")) as PackageManifest;
}

function sameStringRecord(
  actual: Readonly<Record<string, string>> | undefined,
  expected: Readonly<Record<string, string>>,
): boolean {
  if (actual === undefined) {
    return false;
  }
  const expectedEntries = Object.entries(expected);
  return (
    Object.keys(actual).length === expectedEntries.length &&
    expectedEntries.every(([key, value]) => actual[key] === value)
  );
}

function cargoMetadata(): CargoMetadata {
  const result = spawnSync(
    "cargo",
    ["metadata", "--no-deps", "--format-version", "1"],
    {
      cwd: repositoryRoot,
      encoding: "utf8",
      shell: false,
    },
  );
  if (result.error !== undefined) {
    throw new Error(`failed to run cargo metadata: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`cargo metadata failed: ${result.stderr.trim()}`);
  }
  return JSON.parse(result.stdout) as CargoMetadata;
}

async function checkPackageManifests(): Promise<string> {
  const errors: string[] = [];
  const metadata = cargoMetadata();
  const rustCli = metadata.packages.find(
    (candidate) => candidate.name === "tomlsmith-cli",
  );
  if (rustCli === undefined) {
    throw new Error("cargo metadata did not contain tomlsmith-cli");
  }
  if (
    rustCli.publish === null ||
    rustCli.publish.length !== 1 ||
    rustCli.publish[0] !== "crates-io"
  ) {
    errors.push("crates/tomlsmith-cli must publish only to crates-io");
  }

  const cliManifest = await readPackageManifest(
    resolve(repositoryRoot, "npm", "cli", "package.json"),
  );
  if (cliManifest.name !== "@tomlsmith/cli") {
    errors.push("npm/cli/package.json must be named @tomlsmith/cli");
  }
  if (cliManifest.version !== rustCli.version) {
    errors.push(
      `@tomlsmith/cli version ${String(cliManifest.version)} does not match Rust version ${rustCli.version}`,
    );
  }

  const expectedOptionalDependencies = Object.fromEntries(
    nativePlatforms.map(({ packageName }) => [
      packageName,
      `workspace:${rustCli.version}`,
    ]),
  );
  if (
    !sameStringRecord(
      cliManifest.optionalDependencies,
      expectedOptionalDependencies,
    )
  ) {
    errors.push(
      "@tomlsmith/cli optionalDependencies must contain every native workspace package at the exact Rust version",
    );
  }

  for (const nativePlatform of nativePlatforms) {
    const manifest = await readPackageManifest(
      resolve(platformPackageDirectory(nativePlatform), "package.json"),
    );
    if (manifest.name !== nativePlatform.packageName) {
      errors.push(
        `${nativePlatform.directory} package name must be ${nativePlatform.packageName}`,
      );
    }
    if (manifest.version !== rustCli.version) {
      errors.push(
        `${nativePlatform.packageName} version ${String(manifest.version)} does not match Rust version ${rustCli.version}`,
      );
    }
    if (manifest.private !== true) {
      errors.push(
        `${nativePlatform.packageName} must remain a private template and be published only through the staging tool`,
      );
    }
    if (!manifest.files?.includes(`bin/${nativePlatform.binaryName}`)) {
      errors.push(
        `${nativePlatform.packageName} must publish bin/${nativePlatform.binaryName}`,
      );
    }
  }

  if (errors.length > 0) {
    throw new Error(`npm package validation failed:\n- ${errors.join("\n- ")}`);
  }
  return rustCli.version;
}

function packageManagerExecutable(name: "npm" | "pnpm"): string {
  return process.platform === "win32" ? `${name}.cmd` : name;
}

function runPnpmPack(packageDirectory: string, output: string): void {
  const result = spawnSync(
    packageManagerExecutable("pnpm"),
    ["pack", "--pack-destination", output],
    {
      cwd: packageDirectory,
      stdio: "inherit",
      shell: process.platform === "win32",
    },
  );
  if (result.error !== undefined) {
    throw new Error(`failed to run pnpm pack: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`pnpm pack exited with status ${String(result.status)}`);
  }
}

async function packCli(outputDirectory: string): Promise<void> {
  await checkPackageManifests();
  const output = resolve(outputDirectory);
  await mkdir(output, { recursive: true });
  const cliDirectory = resolve(repositoryRoot, "npm", "cli");
  const launcher = resolve(cliDirectory, "dist", "cli.js");
  if (!(await regularFile(launcher))) {
    throw new Error(`npm launcher has not been built: ${launcher}`);
  }
  await copyFile(
    resolve(repositoryRoot, "LICENSE"),
    resolve(cliDirectory, "LICENSE"),
  );
  runPnpmPack(cliDirectory, output);
}

async function packCurrentPlatform(options: PackOptions): Promise<void> {
  await checkPackageManifests();
  const nativePlatform = findNativePlatform(options.platform, options.arch);
  if (nativePlatform === undefined) {
    throw new Error(
      `unsupported platform ${options.platform}-${options.arch}; expected one of ${supportedPlatformLabels()}`,
    );
  }
  const binary = stagedBinaryPath(nativePlatform);
  if (!(await regularFile(binary))) {
    throw new Error(
      `native binary has not been staged for ${nativePlatform.packageName}: ${binary}`,
    );
  }

  const output = resolve(options.output);
  await mkdir(output, { recursive: true });
  const platformDirectory = platformPackageDirectory(nativePlatform);
  const stagingDirectory = await mkdtemp(
    join(tmpdir(), `tomlsmith-${nativePlatform.directory}-`),
  );
  try {
    const manifest = JSON.parse(
      await readFile(resolve(platformDirectory, "package.json"), "utf8"),
    ) as Record<string, unknown>;
    const publishableManifest = { ...manifest };
    delete publishableManifest.private;
    const stagedBinary = resolve(
      stagingDirectory,
      "bin",
      nativePlatform.binaryName,
    );
    await mkdir(dirname(stagedBinary), { recursive: true });
    await copyFile(binary, stagedBinary);
    if (nativePlatform.platform !== "win32") {
      await chmod(stagedBinary, 0o755);
    }
    await copyFile(
      resolve(repositoryRoot, "LICENSE"),
      resolve(stagingDirectory, "LICENSE"),
    );
    await writeFile(
      resolve(stagingDirectory, "package.json"),
      `${JSON.stringify(
        {
          ...publishableManifest,
          os: [nativePlatform.platform],
          cpu: [nativePlatform.arch],
          bin: `bin/${nativePlatform.binaryName}`,
        },
        undefined,
        2,
      )}\n`,
    );
    runPnpmPack(stagingDirectory, output);
  } finally {
    await rm(stagingDirectory, { recursive: true, force: true });
  }

  if (options.includeCli) {
    await packCli(output);
  }
}

async function publishPackages(inputDirectory: string): Promise<void> {
  const version = await checkPackageManifests();
  const input = resolve(inputDirectory);
  const packages = [
    ...nativePlatforms.map((nativePlatform) => ({
      name: nativePlatform.packageName,
      archive: resolve(
        input,
        `tomlsmith-cli-${nativePlatform.directory}-${version}.tgz`,
      ),
    })),
    {
      name: "@tomlsmith/cli",
      archive: resolve(input, `tomlsmith-cli-${version}.tgz`),
    },
  ];

  const archiveChecks = await Promise.all(
    packages.map(async (releasePackage) => ({
      ...releasePackage,
      exists: await regularFile(releasePackage.archive),
    })),
  );
  const missingArchives = archiveChecks.filter(({ exists }) => !exists);
  if (missingArchives.length > 0) {
    throw new Error(
      `release archives are incomplete:\n${missingArchives
        .map(({ name, archive }) => `- ${name}: ${archive}`)
        .join("\n")}`,
    );
  }

  for (const releasePackage of packages) {
    const specifier = `${releasePackage.name}@${version}`;
    const view = spawnSync(
      packageManagerExecutable("npm"),
      ["view", specifier, "dist.integrity", "--json"],
      {
        cwd: repositoryRoot,
        encoding: "utf8",
        shell: process.platform === "win32",
      },
    );
    if (view.status === 0) {
      const publishedIntegrity = JSON.parse(view.stdout) as unknown;
      const localIntegrity = await archiveIntegrity(releasePackage.archive);
      if (publishedIntegrity !== localIntegrity) {
        throw new Error(
          `published ${specifier} has integrity ${String(publishedIntegrity)}, but the local release archive has integrity ${localIntegrity}; refusing to continue`,
        );
      }
      process.stdout.write(
        `already published ${specifier} with matching integrity; skipping\n`,
      );
      continue;
    }
    if (!view.stderr.includes("E404")) {
      throw new Error(
        `failed to query ${specifier} before publishing: ${view.stderr.trim()}`,
      );
    }

    const publish = spawnSync(
      packageManagerExecutable("npm"),
      ["publish", releasePackage.archive, "--access", "public"],
      {
        cwd: repositoryRoot,
        stdio: "inherit",
        shell: process.platform === "win32",
      },
    );
    if (publish.error !== undefined) {
      throw new Error(
        `failed to publish ${specifier}: ${publish.error.message}`,
      );
    }
    if (publish.status !== 0) {
      throw new Error(
        `npm publish failed for ${specifier} with status ${String(publish.status)}`,
      );
    }
  }
}

const program = new Command()
  .name("tomlsmith-npm")
  .description("Build and validate the native npm distribution");

program
  .command("stage")
  .description("copy a native TomlSmith executable into its platform package")
  .requiredOption("--binary <path>", "path to the compiled TomlSmith executable")
  .option("--platform <platform>", "Node platform name", process.platform)
  .option("--arch <arch>", "Node CPU architecture", process.arch)
  .action(async (options: StageOptions) => {
    await stageNativeBinary(options);
  });

program
  .command("check")
  .description("validate version and platform package invariants")
  .action(async () => {
    const version = await checkPackageManifests();
    process.stdout.write(`validated npm package manifests at ${version}\n`);
  });

program
  .command("pack")
  .description("pack the staged native package and optionally the public CLI package")
  .option(
    "--output <directory>",
    "directory for package tarballs",
    resolve(repositoryRoot, "npm", "dist"),
  )
  .option("--platform <platform>", "Node platform name", process.platform)
  .option("--arch <arch>", "Node CPU architecture", process.arch)
  .option("--include-cli", "also pack the platform-neutral @tomlsmith/cli package", false)
  .action(async (options: PackOptions) => {
    await packCurrentPlatform(options);
  });

program
  .command("pack-cli")
  .description("pack the platform-neutral @tomlsmith/cli launcher")
  .option(
    "--output <directory>",
    "directory for the package tarball",
    resolve(repositoryRoot, "npm", "dist"),
  )
  .action(async (options: OutputOptions) => {
    await packCli(options.output);
  });

program
  .command("publish")
  .description("publish complete, prebuilt npm tarballs in dependency order")
  .option(
    "--input <directory>",
    "directory containing all release tarballs",
    resolve(repositoryRoot, "npm", "dist"),
  )
  .action(async (options: PublishOptions) => {
    await publishPackages(options.input);
  });

await program.parseAsync();
