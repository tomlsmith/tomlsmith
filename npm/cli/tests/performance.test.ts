// Release-CLI performance guards over the real process interface.
//
// Two kinds of test share this file. Growth "waterlines" run one workload and
// the same workload scaled by a factor, subtract the measured process start-up
// floor, and assert the corrected time grows by less than twice the linear
// expectation: a quadratic implementation crosses that line while a linear one
// stays well below it. Comparison tests interleave the head binary with the
// comparison-SHA binary (`TOMLSMITH_BASELINE_BIN`) and report the paired median
// ratio; shared runners drift, so ratios are advisory above `adviseAboveRatio`
// and only a gross regression above `blockAboveRatio` fails the job unless
// `TOMLSMITH_PERFORMANCE_BUDGET_MODE=strict`. Output content is asserted only
// on the head binary: the comparison binary may format differently on purpose.
// The complexity contracts themselves are proved in-process by the Rust suite
// `crates/tomlsmith/tests/complexity.rs`; this file checks the shipped CLI.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname } from "node:path";
import { performance } from "node:perf_hooks";

import { afterAll, expect, test } from "vitest";

const require = createRequire(import.meta.url);
const baselineBinaryPath = process.env.TOMLSMITH_BASELINE_BIN?.trim();
const baselineRequired = process.env.TOMLSMITH_REQUIRE_BASELINE === "1";
const budgetMode =
  process.env.TOMLSMITH_PERFORMANCE_BUDGET_MODE === "strict"
    ? "strict"
    : "advisory";
const summaryPath = process.env.TOMLSMITH_PERFORMANCE_SUMMARY?.trim();
const commandTimeoutMilliseconds = 30_000;
const comparisonPairs = 7;
/** Every paired-median ratio above this is reported; strict mode fails on it. */
const adviseAboveRatio = 1.35;
/** A gross regression fails the job in both modes. */
const blockAboveRatio = 3.0;
/** The small growth workload must dominate process start-up by this factor. */
const minimumSmallToFloorRatio = 4;

if (baselineRequired && !baselineBinaryPath) {
  throw new Error(
    "TOMLSMITH_REQUIRE_BASELINE=1 requires TOMLSMITH_BASELINE_BIN",
  );
}

type ComparisonRecord = {
  name: string;
  baseline: number;
  head: number;
  ratio: number;
  mad: number;
  pairedRatios: number[];
  adviseAbove: number;
  blockAbove: number;
  verdict: "ok" | "advisory" | "blocked";
};

type GrowthRecord = {
  name: string;
  floor: number;
  small: number;
  large: number;
  ratio: number;
  limit: number;
};

type MemoryRecord = {
  name: string;
  inputBytes: number;
  peakRssMiB: number;
  budgetMiB: number;
  verdict: "ok" | "blocked";
};

const summary: {
  comparisons: ComparisonRecord[];
  growth: GrowthRecord[];
  memory: MemoryRecord[];
} = {
  comparisons: [],
  growth: [],
  memory: [],
};

afterAll(() => {
  if (summaryPath === undefined || summaryPath.length === 0) {
    return;
  }
  mkdirSync(dirname(summaryPath), { recursive: true });
  writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
});

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
      `the npm performance test has no package for ${process.platform}-${process.arch}`,
    );
  }
  return label;
}

function nativeBinaryPath(): string {
  const binaryName =
    process.platform === "win32" ? "tomlsmith.exe" : "tomlsmith";
  return require.resolve(
    `@tomlsmith/cli-${platformArchiveLabel()}/bin/${binaryName}`,
  );
}

const headBinaryPath = nativeBinaryPath();

/** Output content is a head contract; the comparison binary only has to succeed. */
function assertsOutput(binaryPath: string): boolean {
  return binaryPath === headBinaryPath;
}

function median(values: number[]): number {
  if (values.length === 0) {
    throw new Error("cannot calculate the median of an empty sample");
  }
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[middle - 1]! + sorted[middle]!) / 2
    : sorted[middle]!;
}

function reportRatio(
  name: string,
  baseline: number,
  comparison: number,
): number {
  const ratio = comparison / baseline;
  console.info(
    `[performance] ${name}: ${baseline.toFixed(2)}ms -> ${comparison.toFixed(2)}ms (${ratio.toFixed(2)}x)`,
  );
  return ratio;
}

function runCli(
  binaryPath: string,
  cliArguments: readonly string[],
  source: string,
): { elapsed: number; stdout: string } {
  const started = performance.now();
  const result = spawnSync(binaryPath, [...cliArguments], {
    encoding: "utf8",
    input: source,
    maxBuffer: 128 * 1024 * 1024,
    timeout: commandTimeoutMilliseconds,
  });
  const elapsed = performance.now() - started;
  expect(result.error).toBeUndefined();
  expect(result.status, result.stderr).toBe(0);
  expect(result.stderr).toBe("");
  return { elapsed, stdout: result.stdout };
}

/** Median wall time of the smallest possible check: process start-up plus I/O. */
function spawnFloor(binaryPath: string): number {
  const samples: number[] = [];
  for (let index = 0; index < 9; index += 1) {
    samples.push(
      runCli(binaryPath, ["--toml-version", "1.1", "check", "-"], "a = 1\n")
        .elapsed,
    );
  }
  return median(samples);
}

/**
 * Peak resident set size of one CLI run, measured by the platform's time
 * meter (GNU `time -v` on Linux, BSD `time -l` on macOS). Resident memory is
 * deterministic to within a few percent on shared runners, unlike wall time,
 * so the budgets below block. Returns undefined where no meter is available.
 */
function peakResidentMiB(
  cliArguments: readonly string[],
  source: string,
  expectedStatus = 0,
): number | undefined {
  const meter = "/usr/bin/time";
  const flag =
    process.platform === "linux"
      ? "-v"
      : process.platform === "darwin"
        ? "-l"
        : undefined;
  if (flag === undefined || !existsSync(meter)) {
    return undefined;
  }
  const result = spawnSync(meter, [flag, headBinaryPath, ...cliArguments], {
    encoding: "utf8",
    input: source,
    maxBuffer: 128 * 1024 * 1024,
    timeout: commandTimeoutMilliseconds,
  });
  expect(result.error).toBeUndefined();
  expect(result.status, result.stderr).toBe(expectedStatus);
  const linux = /Maximum resident set size \(kbytes\):\s*(\d+)/.exec(result.stderr);
  if (linux?.[1] !== undefined) {
    return Number(linux[1]) / 1024;
  }
  const darwin = /(\d+)\s+maximum resident set size/.exec(result.stderr);
  if (darwin?.[1] !== undefined) {
    return Number(darwin[1]) / (1024 * 1024);
  }
  return undefined;
}

function assertPeakResidentWithin(
  name: string,
  cliArguments: readonly string[],
  source: string,
  budgetMiB: number,
): void {
  const peakRssMiB = peakResidentMiB(cliArguments, source);
  if (peakRssMiB === undefined) {
    console.warn(
      `[performance] ${name}: no resident-memory meter on ${process.platform}; skipping the memory waterline`,
    );
    return;
  }
  const verdict: MemoryRecord["verdict"] =
    peakRssMiB <= budgetMiB ? "ok" : "blocked";
  console.info(
    `[performance] ${name}: peak RSS ${peakRssMiB.toFixed(1)} MiB for ${source.length} input bytes (budget ${budgetMiB} MiB)`,
  );
  summary.memory.push({
    name,
    inputBytes: source.length,
    peakRssMiB,
    budgetMiB,
    verdict,
  });
  expect(
    peakRssMiB,
    `${name}: peak RSS ${peakRssMiB.toFixed(1)} MiB exceeds the ${budgetMiB} MiB budget`,
  ).toBeLessThanOrEqual(budgetMiB);
}

function measureGrowth(
  name: string,
  measureSmall: () => number,
  measureLarge: () => number,
  limit: number,
): number {
  const floor = spawnFloor(headBinaryPath);
  measureSmall();
  measureLarge();
  const smallSamples: number[] = [];
  const largeSamples: number[] = [];
  for (let pair = 0; pair < 4; pair += 1) {
    const sizes: readonly ("small" | "large")[] =
      pair % 2 === 0 ? ["small", "large"] : ["large", "small"];
    for (const size of sizes) {
      (size === "small" ? smallSamples : largeSamples).push(
        size === "small" ? measureSmall() : measureLarge(),
      );
    }
  }
  const smallRaw = median(smallSamples);
  const largeRaw = median(largeSamples);
  if (smallRaw < floor * minimumSmallToFloorRatio) {
    // A loaded runner can inflate the floor; the sizes below are chosen so
    // this stays rare, and the uncorrected ratio only under-reports growth.
    console.warn(
      `[performance] ${name}: the small workload (${smallRaw.toFixed(2)}ms) is not ${minimumSmallToFloorRatio}x the process start-up floor (${floor.toFixed(2)}ms); growth is measured without start-up correction`,
    );
  }
  // Never remove more than half of a sample: the floor is measured
  // separately and a transient stall must not turn a sample negative.
  const correction = Math.min(floor, smallRaw / 2);
  const small = smallRaw - correction;
  const large = largeRaw - correction;
  const ratio = reportRatio(
    `${name} (start-up ${correction.toFixed(2)}ms removed)`,
    small,
    large,
  );
  summary.growth.push({ name, floor, small, large, ratio, limit });
  return ratio;
}

function representativeSource(): string {
  return Array.from(
    { length: 4096 },
    (_, index) =>
      `[section_${index}]\n` +
      `value = ${index}\n` +
      `when = 1979-05-27 07:32:00Z\n` +
      `metadata = { left = ${index}, right = ${index + 1} }\n` +
      `items = [1, 2, 3, 4]\n`,
  ).join("");
}

function measureRepresentativeCommand(
  binaryPath: string,
  operation: "check" | "format",
  source: string,
): number {
  const operationArguments =
    operation === "check"
      ? ["check", "-"]
      : ["fmt", "--line-width", "65535", "-"];
  const { elapsed, stdout } = runCli(
    binaryPath,
    ["--toml-version", "1.1", ...operationArguments],
    source,
  );
  if (assertsOutput(binaryPath)) {
    if (operation === "check") {
      expect(stdout).toBe("");
    } else {
      expect(stdout).toContain("[section_4095]");
    }
  }
  return elapsed;
}

function compareWithBaseline(
  name: string,
  measure: (binaryPath: string) => number,
): void {
  if (baselineBinaryPath === undefined || baselineBinaryPath.length === 0) {
    throw new Error("the baseline comparison requires TOMLSMITH_BASELINE_BIN");
  }
  measure(baselineBinaryPath);
  measure(headBinaryPath);
  const baselineSamples: number[] = [];
  const headSamples: number[] = [];
  const pairedRatios: number[] = [];
  for (let pair = 0; pair < comparisonPairs; pair += 1) {
    const binaries: readonly {
      kind: "baseline" | "head";
      path: string;
    }[] =
      pair % 2 === 0
        ? [
            { kind: "baseline", path: baselineBinaryPath },
            { kind: "head", path: headBinaryPath },
          ]
        : [
            { kind: "head", path: headBinaryPath },
            { kind: "baseline", path: baselineBinaryPath },
          ];
    let baselineElapsed = 0;
    let headElapsed = 0;
    for (const binary of binaries) {
      const elapsed = measure(binary.path);
      (binary.kind === "baseline" ? baselineSamples : headSamples).push(
        elapsed,
      );
      if (binary.kind === "baseline") {
        baselineElapsed = elapsed;
      } else {
        headElapsed = elapsed;
      }
    }
    pairedRatios.push(headElapsed / baselineElapsed);
  }
  const baseline = median(baselineSamples);
  const head = median(headSamples);
  const ratio = median(pairedRatios);
  const medianAbsoluteDeviation = median(
    pairedRatios.map((sample) => Math.abs(sample - ratio)),
  );
  reportRatio(`${name} head versus comparison SHA`, baseline, head);
  console.info(
    `[performance] ${name} paired ratios: ${pairedRatios.map((sample) => sample.toFixed(3)).join(", ")} (median ${ratio.toFixed(3)}, MAD ${medianAbsoluteDeviation.toFixed(3)})`,
  );
  const verdict: ComparisonRecord["verdict"] =
    ratio >= blockAboveRatio
      ? "blocked"
      : ratio >= adviseAboveRatio
        ? "advisory"
        : "ok";
  summary.comparisons.push({
    name,
    baseline,
    head,
    ratio,
    mad: medianAbsoluteDeviation,
    pairedRatios,
    adviseAbove: adviseAboveRatio,
    blockAbove: blockAboveRatio,
    verdict,
  });
  if (verdict === "advisory") {
    console.warn(
      `[performance] ADVISORY: ${name} is ${ratio.toFixed(2)}x the comparison SHA (advisory above ${adviseAboveRatio}x; shared runners drift, re-run before acting)`,
    );
  }
  expect(
    ratio,
    `${name} became ${ratio.toFixed(2)}x slower than the comparison SHA (gross-regression budget ${blockAboveRatio}x)`,
  ).toBeLessThan(blockAboveRatio);
  if (budgetMode === "strict") {
    expect(
      ratio,
      `${name} became ${ratio.toFixed(2)}x slower than the comparison SHA (strict budget ${adviseAboveRatio}x)`,
    ).toBeLessThan(adviseAboveRatio);
  }
}

function measureIndependentInlineTables(
  count: number,
  lineWidth: number,
  binaryPath = headBinaryPath,
): number {
  const source = Array.from(
    { length: count },
    (_, index) => `item_${index}={left=${index},right=${index + 1}}\n`,
  ).join("");
  const { elapsed, stdout } = runCli(
    binaryPath,
    [
      "--toml-version",
      "1.1",
      "fmt",
      "--line-width",
      lineWidth.toString(),
      "-",
    ],
    source,
  );
  if (assertsOutput(binaryPath)) {
    const expectedFirst =
      lineWidth === 65_535
        ? "item_0 = { left = 0, right = 1 }"
        : "item_0 = {\n  left = 0,\n  right = 1\n}";
    const expectedLast =
      lineWidth === 65_535
        ? `item_${count - 1} = { left = ${count - 1}, right = ${count} }`
        : `item_${count - 1} = {\n  left = ${count - 1},\n  right = ${count}\n}`;
    expect(stdout).toContain(expectedFirst);
    expect(stdout).toContain(expectedLast);
  }
  return elapsed;
}

function measureIndependentArrayTables(
  count: number,
  binaryPath = headBinaryPath,
): number {
  const source = Array.from(
    { length: count },
    (_, index) => `[[table_${index}]]\nvalue = ${index}\n`,
  ).join("");
  const { elapsed, stdout } = runCli(
    binaryPath,
    ["--toml-version", "1.1", "check", "-"],
    source,
  );
  if (assertsOutput(binaryPath)) {
    expect(stdout).toBe("");
  }
  return elapsed;
}

function measureConsecutiveBlankLines(
  count: number,
  binaryPath = headBinaryPath,
): number {
  const source = `head = 1\n${"\n".repeat(count)}[tail]\nvalue = 2\n`;
  const { elapsed, stdout } = runCli(
    binaryPath,
    ["--toml-version", "1.1", "fmt", "-"],
    source,
  );
  if (assertsOutput(binaryPath)) {
    expect(stdout).toBe("head = 1\n\n[tail]\nvalue = 2\n");
  }
  return elapsed;
}

function measureNestedInlineTables(
  depth: number,
  chainCount = 64,
  binaryPath = headBinaryPath,
): number {
  const chain = `${"{ value = ".repeat(depth)}0${" }".repeat(depth)}`;
  const source = Array.from(
    { length: chainCount },
    (_, index) => `root_${index} = ${chain}\n`,
  ).join("");
  const { elapsed, stdout } = runCli(
    binaryPath,
    ["--toml-version", "1.1", "fmt", "--line-width", "20", "-"],
    source,
  );
  if (assertsOutput(binaryPath)) {
    expect(stdout).toMatch(/^root_0 = \{\n  value = \{/);
    expect(stdout).toContain(`root_${chainCount - 1} = {`);
    expect(stdout).toMatch(/\n\}\n$/);
  }
  return elapsed;
}

function measureLargeArray(
  elementCount: number,
  binaryPath = headBinaryPath,
): number {
  const row = Array(elementCount).fill("0").join(",");
  const source = Array.from(
    { length: 16 },
    (_, index) => `value_${index} = [${row}]\n`,
  ).join("");
  const { elapsed, stdout } = runCli(
    binaryPath,
    ["--toml-version", "1.1", "fmt", "--line-width", "65535", "-"],
    source,
  );
  if (assertsOutput(binaryPath)) {
    expect(stdout).toMatch(/^value_0 = \[0, 0/);
    expect(stdout).toMatch(/value_15 = \[0, 0/);
    expect(stdout).toMatch(/0\]\n$/);
  }
  return elapsed;
}

test(
  "checking independent array tables scales below the quadratic regression waterline",
  () => {
    const ratio = measureGrowth(
      "check independent AoT (8x input)",
      () => measureIndependentArrayTables(32_768),
      () => measureIndependentArrayTables(262_144),
      16,
    );

    expect(
      ratio,
      `checking 8x more array tables took ${ratio.toFixed(1)}x longer`,
    ).toBeLessThan(16);
  },
  120_000,
);

test(
  "formatting independent inline tables scales below the quadratic regression waterline",
  () => {
    const ratio = measureGrowth(
      "format independent inline tables (8x input)",
      () => measureIndependentInlineTables(16_384, 65_535),
      () => measureIndependentInlineTables(131_072, 65_535),
      16,
    );

    expect(
      ratio,
      `formatting 8x more inline tables took ${ratio.toFixed(1)}x longer`,
    ).toBeLessThan(16);
  },
  120_000,
);

test(
  "expanding independent inline tables scales below the quadratic regression waterline",
  () => {
    const growthRatio = measureGrowth(
      "format expanded inline tables (8x input)",
      () => measureIndependentInlineTables(16_384, 24),
      () => measureIndependentInlineTables(131_072, 24),
      16,
    );
    const largeSamples: number[] = [];
    const flatLargeSamples: number[] = [];
    for (let sample = 0; sample < 4; sample += 1) {
      const lineWidths = sample % 2 === 0 ? [24, 65_535] : [65_535, 24];
      for (const lineWidth of lineWidths) {
        const elapsed = measureIndependentInlineTables(131_072, lineWidth);
        (lineWidth === 24 ? largeSamples : flatLargeSamples).push(elapsed);
      }
    }
    const layoutRatio = reportRatio(
      "format expanded versus flat inline tables",
      median(flatLargeSamples),
      median(largeSamples),
    );

    expect(
      growthRatio,
      `expanding 8x more inline tables took ${growthRatio.toFixed(1)}x longer`,
    ).toBeLessThan(16);
    expect(
      layoutRatio,
      `expanding inline tables took ${layoutRatio.toFixed(2)}x the flat-layout time`,
    ).toBeLessThan(2.0);
  },
  120_000,
);

test(
  "formatting consecutive blank lines scales below the quadratic regression waterline",
  () => {
    const ratio = measureGrowth(
      "format blank-line run (4x input)",
      () => measureConsecutiveBlankLines(2_097_152),
      () => measureConsecutiveBlankLines(8_388_608),
      8,
    );

    expect(
      ratio,
      `formatting 4x more blank lines took ${ratio.toFixed(1)}x longer`,
    ).toBeLessThan(8);
  },
  120_000,
);

test(
  "formatting nested inline tables scales with produced layout instead of repeated document passes",
  () => {
    const ratio = measureGrowth(
      "format nested inline tables (4x depth)",
      // Depth 256 with 256 chains writes about 34 MB of expanded layout.
      () => measureNestedInlineTables(64, 256),
      () => measureNestedInlineTables(256, 256),
      12,
    );

    // Expanded output grows with depth squared (each level indents every
    // deeper line), so time may grow faster than the input; a repeated
    // document pass would multiply that again.
    expect(
      ratio,
      `formatting 4x deeper inline tables took ${ratio.toFixed(1)}x longer`,
    ).toBeLessThan(12);
  },
  120_000,
);

test(
  "formatting a large array does not rescan the produced line for every comma",
  () => {
    const ratio = measureGrowth(
      "format large arrays (4x input)",
      () => measureLargeArray(32_768),
      () => measureLargeArray(131_072),
      8,
    );

    expect(
      ratio,
      `formatting 4x more array elements took ${ratio.toFixed(1)}x longer`,
    ).toBeLessThan(8);
  },
  120_000,
);

test.skipIf(!baselineBinaryPath)(
  "representative check stays within the comparison SHA budget",
  () => {
    const source = representativeSource();
    compareWithBaseline("representative check", (binaryPath) =>
      measureRepresentativeCommand(binaryPath, "check", source),
    );
  },
  120_000,
);

test.skipIf(!baselineBinaryPath)(
  "representative format stays within the comparison SHA budget",
  () => {
    const source = representativeSource();
    compareWithBaseline("representative format", (binaryPath) =>
      measureRepresentativeCommand(binaryPath, "format", source),
    );
  },
  120_000,
);

test.skipIf(!baselineBinaryPath)(
  "array-table check stays within the comparison SHA budget",
  () => {
    compareWithBaseline("array-table check", (binaryPath) =>
      measureIndependentArrayTables(8192, binaryPath),
    );
  },
  120_000,
);

test.skipIf(!baselineBinaryPath)(
  "expanded inline-table format stays within the comparison SHA budget",
  () => {
    compareWithBaseline("expanded inline-table format", (binaryPath) =>
      measureIndependentInlineTables(4096, 24, binaryPath),
    );
  },
  120_000,
);

test.skipIf(!baselineBinaryPath)(
  "blank-line format stays within the comparison SHA budget",
  () => {
    compareWithBaseline("blank-line format", (binaryPath) =>
      measureConsecutiveBlankLines(32_768, binaryPath),
    );
  },
  120_000,
);

test.skipIf(!baselineBinaryPath)(
  "nested inline-table format stays within the comparison SHA budget",
  () => {
    compareWithBaseline("nested inline-table format", (binaryPath) =>
      measureNestedInlineTables(128, 8, binaryPath),
    );
  },
  120_000,
);

test.skipIf(!baselineBinaryPath)(
  "large-array format stays within the comparison SHA budget",
  () => {
    compareWithBaseline("large-array format", (binaryPath) =>
      measureLargeArray(8192, binaryPath),
    );
  },
  120_000,
);

// Memory waterlines. Budgets are about 2x the values measured on the
// governed release build (representative 400 KiB: check 16 MiB, format
// 18 MiB; 16 384 expanded inline tables: 22 MiB; refused 10 000-deep
// nesting: 8 MiB), so they fail on a regression to retained token tapes,
// per-token prefix arrays, or rendering before refusal, not on runner noise.
test("check and format stay within their resident-memory budgets", () => {
  const source = representativeSource();
  assertPeakResidentWithin(
    "representative check",
    ["--toml-version", "1.1", "check", "-"],
    source,
    40,
  );
  assertPeakResidentWithin(
    "representative format",
    ["--toml-version", "1.1", "fmt", "--line-width", "65535", "-"],
    source,
    40,
  );
  const tables = Array.from(
    { length: 16_384 },
    (_, index) => `item_${index}={left=${index},right=${index + 1}}\n`,
  ).join("");
  assertPeakResidentWithin(
    "expanded inline-table format",
    ["--toml-version", "1.1", "fmt", "--line-width", "24", "-"],
    tables,
    48,
  );
  const depth = 10_000;
  const refused = `a = ${"{ b = ".repeat(depth)}1${" }".repeat(depth)}\n`;
  const result = spawnSync(
    headBinaryPath,
    ["--toml-version", "1.1", "fmt", "--line-width", "20", "-"],
    { encoding: "utf8", input: refused, timeout: commandTimeoutMilliseconds },
  );
  expect(result.status).toBe(1);
  expect(result.stderr).toContain("parse.nesting-limit");
  const peak = peakResidentMiB(
    ["--toml-version", "1.1", "check", "-"],
    refused,
    1,
  );
  if (peak !== undefined) {
    summary.memory.push({
      name: "refused deep nesting check",
      inputBytes: refused.length,
      peakRssMiB: peak,
      budgetMiB: 24,
      verdict: peak <= 24 ? "ok" : "blocked",
    });
    expect(peak, `refused deep nesting used ${peak.toFixed(1)} MiB`).toBeLessThanOrEqual(24);
  }
}, 120_000);
