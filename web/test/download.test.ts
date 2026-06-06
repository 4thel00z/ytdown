import { describe, it, expect, vi, afterEach } from "vitest";
import { downloadToDisk } from "../src/download";

const body = () =>
  new ReadableStream<Uint8Array>({
    start(c) {
      c.enqueue(new Uint8Array([1, 2]));
      c.enqueue(new Uint8Array([3, 4]));
      c.close();
    },
  });

describe("downloadToDisk", () => {
  afterEach(() => { vi.unstubAllGlobals(); });

  it("streams to a WritableStream via File System Access when available", async () => {
    const chunks: Uint8Array[] = [];
    const writable = {
      write: vi.fn(async (c: Uint8Array) => void chunks.push(c)),
      close: vi.fn(async () => {}),
    };
    const handle = { createWritable: vi.fn(async () => writable) };
    vi.stubGlobal("showSaveFilePicker", vi.fn(async () => handle));
    const fetchMock = vi.fn(async () => new Response(body()));
    vi.stubGlobal("fetch", fetchMock);

    await downloadToDisk("https://cdn/v.mp4", { filename: "v.mp4" });

    expect((globalThis as any).showSaveFilePicker).toHaveBeenCalled();
    expect(writable.close).toHaveBeenCalled();
    expect(chunks.length).toBe(2);
  });

  it("falls back to a Blob + anchor when FS Access is missing", async () => {
    vi.stubGlobal("showSaveFilePicker", undefined);
    const click = vi.fn();
    const anchor = { href: "", download: "", click, remove: vi.fn() } as any;
    vi.spyOn(document, "createElement").mockReturnValue(anchor);
    vi.stubGlobal("URL", { createObjectURL: () => "blob:x", revokeObjectURL: vi.fn() } as any);
    const fetchMock = vi.fn(async () => new Response(body()));
    vi.stubGlobal("fetch", fetchMock);

    await downloadToDisk("https://cdn/v.mp4", { filename: "v.mp4" });

    expect(click).toHaveBeenCalled();
    expect(anchor.download).toBe("v.mp4");
  });
});
