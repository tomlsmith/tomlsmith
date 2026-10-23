import { performance } from "node:perf_hooks";
import { expect, test } from "vitest";
import { LspClient, type SemanticTokens } from "./client.js";

test("semantic highlighting grows within a linear budget for long ASCII and Unicode lines", async () => {
  const client = new LspClient("release");
  try {
    await client.initialize();
    for (const literal of ["1", '"😀漢"']) {
      const times: number[] = [];
      for (const count of [8_000, 32_000]) {
        const uri = `file:///long-${literal === "1" ? "ascii" : "unicode"}-${count}.toml`;
        await client.open(uri, `values = [${Array.from({ length: count }, () => literal).join(",")}]\n`);
        const request = () => client.request<SemanticTokens>("textDocument/semanticTokens/full", { textDocument: { uri } });
        await request();
        const samples = [];
        for (let iteration = 0; iteration < 3; iteration++) {
          const start = performance.now();
          const tokens = await request();
          samples.push(performance.now() - start);
          expect(tokens.data.length).toBeGreaterThan(count * 5);
        }
        times.push(samples.sort((a, b) => a - b)[1]!);
        client.notify("textDocument/didClose", { textDocument: { uri } });
      }
      console.info({ literal, smallMs: times[0], largeMs: times[1], ratio: times[1]! / times[0]! });
      // 4x input allows 8x time plus transport jitter, but rejects the old 16x prefix scans.
      expect(times[1]!).toBeLessThan(times[0]! * 8 + 30);
    }
  } finally {
    await client.close();
  }
}, 60_000);
