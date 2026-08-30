#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";

import {
  findNativePlatform,
  supportedPlatformLabels,
} from "../../platforms";

const OPERATIONAL_FAILURE = 2;

function reportOperationalFailure(message: string): void {
  process.stderr.write(`tomlsmith: ${message}\n`);
  process.exitCode = OPERATIONAL_FAILURE;
}

function main(): void {
  const nativePlatform = findNativePlatform(process.platform, process.arch);
  if (nativePlatform === undefined) {
    reportOperationalFailure(
      `no native binary is published for ${process.platform}-${process.arch}; supported platforms: ${supportedPlatformLabels()}`,
    );
    return;
  }

  const require = createRequire(import.meta.url);
  let binaryPath: string;
  try {
    binaryPath = require.resolve(
      `${nativePlatform.packageName}/bin/${nativePlatform.binaryName}`,
    );
  } catch {
    reportOperationalFailure(
      `the required native platform package is missing (${nativePlatform.packageName}); reinstall @tomlsmith/cli without --omit=optional`,
    );
    return;
  }

  const result = spawnSync(binaryPath, process.argv.slice(2), {
    shell: false,
    stdio: "inherit",
  });

  if (result.error !== undefined) {
    reportOperationalFailure(
      `failed to start the native executable at ${binaryPath}: ${result.error.message}`,
    );
    return;
  }

  if (result.signal !== null) {
    try {
      process.kill(process.pid, result.signal);
      return;
    } catch {
      reportOperationalFailure(
        `the native executable terminated with signal ${result.signal}`,
      );
      return;
    }
  }

  process.exitCode = result.status ?? OPERATIONAL_FAILURE;
}

main();
