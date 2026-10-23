import { expect, test } from "vitest";
import { absoluteTokens, LspClient, type SemanticTokens } from "./client.js";

test("semantic token positions and incremental edits retain UTF-16 coordinates across Unicode and CRLF", async () => {
  const client = new LspClient();
  try {
    await client.initialize();
    const uri = "file:///unicode.toml";
    const lines = ['"😀鍵" = ["é", "𝄞", 123] # 注释', 'message = """α', '😀漢"""', 'tail = 42'];
    await client.open(uri, `${lines.join("\r\n")}\r\n`);
    const tokens = absoluteTokens(await client.request<SemanticTokens>("textDocument/semanticTokens/full", { textDocument: { uri } }));
    expect(tokens).toContainEqual({ line: 0, start: lines[0]!.indexOf("123"), length: 3, type: 3 });
    expect(tokens).toContainEqual({ line: 0, start: lines[0]!.indexOf('"𝄞"'), length: 4, type: 2 });
    expect(tokens).toContainEqual({ line: 2, start: 0, length: 6, type: 2 });
    expect(tokens).toContainEqual({ line: 3, start: 7, length: 2, type: 3 });

    const character = lines[0]!.indexOf("123");
    const diagnostics = client.notification("textDocument/publishDiagnostics", (params) =>
      (params as { version?: number }).version === 2,
    );
    client.notify("textDocument/didChange", {
      textDocument: { uri, version: 2 },
      contentChanges: [{ range: { start: { line: 0, character }, end: { line: 0, character: character + 3 } }, text: "false" }],
    });
    expect((await diagnostics as { diagnostics: unknown[] }).diagnostics).toEqual([]);
    const changed = absoluteTokens(await client.request<SemanticTokens>("textDocument/semanticTokens/full", { textDocument: { uri } }));
    expect(changed).toContainEqual({ line: 0, start: character, length: 5, type: 4 });
    expect(changed).toContainEqual({ line: 3, start: 7, length: 2, type: 3 });
  } finally {
    await client.close();
  }
}, 30_000);

type Position = { line: number; character: number };
type SymbolInformation = { name: string; location: { range: { start: Position; end: Position } } };

async function change(
  client: LspClient,
  uri: string,
  version: number,
  start: Position,
  end: Position,
  text: string,
): Promise<void> {
  const diagnostics = client.notification("textDocument/publishDiagnostics", (params) => {
    const published = params as { uri: string; version?: number };
    return published.uri === uri && published.version === version;
  });
  client.notify("textDocument/didChange", {
    textDocument: { uri, version },
    contentChanges: [{ range: { start, end }, text }],
  });
  expect((await diagnostics as { diagnostics: unknown[] }).diagnostics).toEqual([]);
}

function symbols(client: LspClient, uri: string): Promise<SymbolInformation[]> {
  return client.request("textDocument/documentSymbol", { textDocument: { uri } });
}

test("an edit inside a surrogate pair rounds up after the full scalar", async () => {
  const client = new LspClient();
  try {
    await client.initialize();
    const uri = "file:///surrogate.toml";
    await client.open(uri, '"é"=0\r\n"😀x"=1\r\nlast=2\r\n');
    // Character 2 is between the UTF-16 code units of 😀, after the quote.
    await change(client, uri, 2, { line: 1, character: 2 }, { line: 1, character: 2 }, "z");
    const result = await symbols(client, uri);
    expect(result.map((symbol) => symbol.name)).toEqual(['"é"', '"😀zx"', "last"]);
    expect(result[1]!.location.range).toEqual({
      start: { line: 1, character: 0 },
      end: { line: 1, character: '"😀zx"=1'.length },
    });
  } finally {
    await client.close();
  }
}, 10_000);

test("oversized columns stop before CRLF and oversized lines clamp to document end", async () => {
  const client = new LspClient();
  try {
    await client.initialize();
    const uri = "file:///clamped-unicode.toml";
    await client.open(uri, '"😀"=1\r\ntail=2\r\n');
    const column = { line: 0, character: 0xffff_ffff };
    await change(client, uri, 2, column, column, "\r\nadded=3");
    const first = await symbols(client, uri);
    expect(first.map((symbol) => symbol.name)).toEqual(['"😀"', "added", "tail"]);
    expect(first.map((symbol) => symbol.location.range.start.line)).toEqual([0, 1, 2]);

    const beyond = { line: 0xffff_ffff, character: 0xffff_ffff };
    await change(client, uri, 3, beyond, beyond, "end=4\r\n");
    const result = await symbols(client, uri);
    expect(result.map((symbol) => symbol.name)).toEqual(['"😀"', "added", "tail", "end"]);
    expect(result[3]!.location.range.start).toEqual({ line: 3, character: 0 });
  } finally {
    await client.close();
  }
}, 10_000);

test("cross-line edits use UTF-16 offsets after non-ASCII text and CRLF", async () => {
  const client = new LspClient();
  try {
    await client.initialize();
    const uri = "file:///cross-line.toml";
    await client.open(uri, '"😀"=1\r\nmiddle="漢"\r\ntail=3\r\n');
    await change(
      client,
      uri,
      2,
      { line: 0, character: '"😀"=1'.length },
      { line: 2, character: "tail=".length },
      "\r\nreplacement=",
    );
    const result = await symbols(client, uri);
    expect(result.map((symbol) => symbol.name)).toEqual(['"😀"', "replacement"]);
    const tokens = absoluteTokens(await client.request<SemanticTokens>(
      "textDocument/semanticTokens/full", { textDocument: { uri } },
    ));
    expect(tokens).toContainEqual({ line: 1, start: "replacement=".length, length: 1, type: 3 });
    expect(result[1]!.location.range.end).toEqual({ line: 1, character: "replacement=3".length });
  } finally {
    await client.close();
  }
}, 10_000);
