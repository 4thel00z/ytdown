import { describe, it, expect, vi } from "vitest";
import { Ytdown } from "../src/index";

describe("Ytdown", () => {
  it("wires the proxy fetch into the wasm core and resolves", async () => {
    const resolve = vi.fn(async (_url: string) => ({
      kind: "single",
      video: { id: "abc", title: "T", formats: [] },
    }));
    const WasmCtor = vi.fn().mockImplementation(() => ({ resolve }));
    const loadWasm = vi.fn(async () => ({ Ytdown: WasmCtor, default: vi.fn() }));

    const yt = await Ytdown.create({ proxy: "https://proxy.example/p", loadWasm });
    const info = await yt.resolve("https://youtu.be/abc");

    expect(WasmCtor).toHaveBeenCalledOnce();
    expect(typeof WasmCtor.mock.calls[0][0]).toBe("function"); // the proxy fetch cb
    expect(resolve).toHaveBeenCalledWith("https://youtu.be/abc");
    expect(info).toMatchObject({ kind: "single" });
  });
});
