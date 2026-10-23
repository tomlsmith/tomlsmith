import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import { LspClient } from "./client.js";

function expectRejected(results: PromiseSettledResult<unknown>[], pattern: RegExp): void {
  for (const result of results) {
    expect(result.status).toBe("rejected");
    if (result.status === "rejected") expect(String(result.reason)).toMatch(pattern);
  }
}

test("a failed spawn rejects requests and notifications and cleanup completes", async () => {
  const missing = fileURLToPath(new URL("./missing-tomlsmith-lsp", import.meta.url));
  const client = new LspClient("debug", missing);
  try {
    const results = await Promise.allSettled([
      client.initialize(),
      client.notification("never", () => true),
    ]);
    expectRejected(results, /ENOENT/u);
  } finally {
    await client.close();
  }
  await expect(client.close()).resolves.toBeUndefined();
}, 5_000);

test("an early process exit settles pending work before cleanup", async () => {
  const client = new LspClient();
  try {
    await client.initialize();
    const notification = client.notification("never", () => true);
    client.notify("exit", null);
    const request = client.request("shutdown", null);
    const results = await Promise.allSettled([notification, request]);
    expectRejected(results, /closed|EPIPE|destroyed/iu);
    await expect(client.request("shutdown", null)).rejects.toThrow();
  } finally {
    await client.close();
  }
  expect(client.process.exitCode !== null || client.process.signalCode !== null).toBe(true);
}, 5_000);

test("a broken input stream rejects notifications and releases the child", async () => {
  const client = new LspClient();
  try {
    await client.initialize();
    const notification = client.notification("never", () => true);
    client.process.stdin.destroy(new Error("write EPIPE"));
    await expect(notification).rejects.toThrow("EPIPE");
    await expect(client.request("shutdown", null)).rejects.toThrow("EPIPE");
  } finally {
    await client.close();
  }
  expect(client.process.exitCode !== null || client.process.signalCode !== null).toBe(true);
}, 5_000);

test("normal cleanup waits for the server to exit successfully", async () => {
  const client = new LspClient();
  try {
    await client.initialize();
  } finally {
    await client.close();
  }
  expect(client.process.exitCode).toBe(0);
  expect(client.process.signalCode).toBeNull();
}, 5_000);
