import { chmod, mkdir, rm } from "node:fs/promises";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const entryPoint = fileURLToPath(
  new URL("../cli/src/cli.ts", import.meta.url),
);
const output = fileURLToPath(new URL("../cli/dist/cli.js", import.meta.url));

await rm(dirname(output), { recursive: true, force: true });
await mkdir(dirname(output), { recursive: true });
await build({
  entryPoints: [entryPoint],
  outfile: output,
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node22",
  logLevel: "info",
});
await chmod(output, 0o755);
