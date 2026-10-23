import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { fileURLToPath } from "node:url";

type Message = {
  id?: number;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: unknown;
};

type NotificationWaiter = {
  deliver: (message: Message) => boolean;
  fail: (error: Error) => void;
};

export class LspClient {
  readonly process: ChildProcessWithoutNullStreams;
  #buffer = Buffer.alloc(0);
  #nextId = 1;
  #pending = new Map<number, (message: Message) => void>();
  #notifications: Message[] = [];
  #listeners = new Set<NotificationWaiter>();
  #failure: Error | undefined;
  #closed = false;
  #closedPromise: Promise<void>;
  #closing: Promise<void> | undefined;

  constructor(profile: "debug" | "release" = "debug", executablePath?: string) {
    const executable = process.platform === "win32" ? "tomlsmith-lsp.exe" : "tomlsmith-lsp";
    const path = executablePath ?? fileURLToPath(new URL(`../../../target/${profile}/${executable}`, import.meta.url));
    this.process = spawn(path, [], { stdio: "pipe" });
    this.#closedPromise = new Promise((resolve) => {
      this.process.once("close", (code, signal) => {
        this.#closed = true;
        this.#fail(new Error(`LSP process closed (${signal ?? code})`));
        resolve();
      });
    });
    this.process.stdout.on("data", (chunk: Buffer) => {
      try {
        this.#buffer = Buffer.concat([this.#buffer, chunk]);
        while (true) {
          const headerEnd = this.#buffer.indexOf("\r\n\r\n");
          if (headerEnd < 0) return;
          const length = Number(/Content-Length: (\d+)/iu.exec(this.#buffer.subarray(0, headerEnd).toString())?.[1]);
          if (!Number.isSafeInteger(length) || length < 0) throw new Error("invalid LSP frame");
          const end = headerEnd + 4 + length;
          if (this.#buffer.length < end) return;
          const message = JSON.parse(this.#buffer.subarray(headerEnd + 4, end).toString()) as Message;
          this.#buffer = this.#buffer.subarray(end);
          if (message.id !== undefined) {
            this.#pending.get(message.id)?.(message);
          } else {
            let consumed = false;
            for (const listener of this.#listeners) {
              consumed = listener.deliver(message) || consumed;
            }
            if (!consumed) this.#notifications.push(message);
          }
        }
      } catch (error) {
        this.#fail(error instanceof Error ? error : new Error(String(error)));
      }
    });
    this.process.stderr.resume();
    this.process.on("error", (error) => this.#fail(error));
    this.process.stdin.on("error", (error) => this.#fail(error));
  }

  notify(method: string, params: unknown): void {
    this.#send({ jsonrpc: "2.0", method, params });
  }

  request<T>(method: string, params: unknown, timeoutMilliseconds = 15_000): Promise<T> {
    if (this.#failure !== undefined) return Promise.reject(this.#failure);
    const id = this.#nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`LSP request timed out: ${method}`));
      }, timeoutMilliseconds);
      const settle = (message: Message) => {
        clearTimeout(timer);
        this.#pending.delete(id);
        if (message.error !== undefined) {
          reject(message.error instanceof Error ? message.error : new Error(JSON.stringify(message.error)));
        } else resolve(message.result as T);
      };
      this.#pending.set(id, settle);
      try {
        this.#send({ jsonrpc: "2.0", id, method, params });
      } catch (error) {
        this.#fail(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  async initialize(): Promise<void> {
    await this.request("initialize", { capabilities: {} });
    this.notify("initialized", {});
  }

  async open(uri: string, text: string): Promise<void> {
    const diagnostics = this.notification("textDocument/publishDiagnostics", (params) =>
      (params as { uri: string }).uri === uri,
    );
    try {
      this.notify("textDocument/didOpen", { textDocument: { uri, languageId: "toml", version: 1, text } });
    } catch (error) {
      this.#fail(error instanceof Error ? error : new Error(String(error)));
    }
    await diagnostics;
  }

  notification(method: string, matches: (params: unknown) => boolean): Promise<unknown> {
    if (this.#failure !== undefined) return Promise.reject(this.#failure);
    const existing = this.#notifications.findIndex((message) => message.method === method && matches(message.params));
    if (existing >= 0) return Promise.resolve(this.#notifications.splice(existing, 1)[0]?.params);
    return new Promise((resolve, reject) => {
      const remove = () => {
        clearTimeout(timer);
        this.#listeners.delete(listener);
      };
      const listener: NotificationWaiter = {
        deliver: (message) => {
          if (message.method !== method || !matches(message.params)) return false;
          remove();
          resolve(message.params);
          return true;
        },
        fail: (error) => {
          remove();
          reject(error);
        },
      };
      const timer = setTimeout(() => listener.fail(new Error(`LSP notification timed out: ${method}`)), 15_000);
      this.#listeners.add(listener);
    });
  }

  close(): Promise<void> {
    this.#closing ??= this.#close();
    return this.#closing;
  }

  async #close(): Promise<void> {
    if (this.#closed) return;
    // Cleanup must not replace the assertion or transport error that brought
    // a test into its finally block. Give graceful shutdown a bounded chance.
    if (this.#failure === undefined) {
      try {
        await this.request("shutdown", null, 1_000);
        this.notify("exit", null);
        this.process.stdin.end();
        if (await this.#waitForClose(1_000)) return;
      } catch {
        // A failed transport or shutdown falls through to process cleanup.
      }
    }
    this.#fail(new Error("LSP client closed"));
    this.process.stdin.destroy();
    try {
      if (!this.#closed) this.process.kill();
      if (await this.#waitForClose(1_000)) return;
      this.process.kill("SIGKILL");
      await this.#waitForClose(1_000);
    } catch {
      // The process may already have exited between the state check and kill.
    }
  }

  #waitForClose(timeoutMilliseconds: number): Promise<boolean> {
    if (this.#closed) return Promise.resolve(true);
    return new Promise((resolve) => {
      const timer = setTimeout(() => resolve(false), timeoutMilliseconds);
      void this.#closedPromise.then(() => {
        clearTimeout(timer);
        resolve(true);
      });
    });
  }

  #fail(error: Error): void {
    this.#failure ??= error;
    for (const callback of this.#pending.values()) callback({ error: this.#failure });
    for (const listener of this.#listeners) listener.fail(this.#failure);
    this.#notifications = [];
  }

  #send(message: unknown): void {
    if (this.#failure !== undefined) throw this.#failure;
    const body = Buffer.from(JSON.stringify(message));
    this.process.stdin.write(Buffer.concat([
      Buffer.from(`Content-Length: ${body.length}\r\n\r\n`),
      body,
    ]));
  }
}

export type SemanticTokens = { data: number[] };

export function absoluteTokens({ data }: SemanticTokens): Array<{ line: number; start: number; length: number; type: number }> {
  let line = 0;
  let start = 0;
  const result = [];
  for (let index = 0; index < data.length; index += 5) {
    const deltaLine = data[index]!;
    line += deltaLine;
    start = (deltaLine === 0 ? start : 0) + data[index + 1]!;
    result.push({ line, start, length: data[index + 2]!, type: data[index + 3]! });
  }
  return result;
}
