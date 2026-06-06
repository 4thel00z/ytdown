import { describe, it, expect, vi } from "vitest";
import worker from "../src/worker";

describe("proxy worker", () => {
  it("forwards to ?url target and adds CORS headers", async () => {
    const upstream = new Response("ok", { status: 200, headers: { "x-up": "1" } });
    const fetchMock = vi.fn<[string | URL | Request, RequestInit?], Promise<Response>>(async () => upstream);
    vi.stubGlobal("fetch", fetchMock);

    const req = new Request("https://w/p?url=" + encodeURIComponent("https://yt/api"), {
      method: "POST",
      body: "{}",
    });
    const res = await worker.fetch(req);

    expect(fetchMock).toHaveBeenCalled();
    expect(String(fetchMock.mock.calls[0][0])).toBe("https://yt/api");
    expect(res.headers.get("access-control-allow-origin")).toBe("*");
    expect(res.status).toBe(200);
  });

  it("answers CORS preflight", async () => {
    const res = await worker.fetch(new Request("https://w/p", { method: "OPTIONS" }));
    expect(res.status).toBe(204);
    expect(res.headers.get("access-control-allow-methods")).toContain("POST");
  });

  it("rejects a missing url param", async () => {
    const res = await worker.fetch(new Request("https://w/p"));
    expect(res.status).toBe(400);
  });
});
