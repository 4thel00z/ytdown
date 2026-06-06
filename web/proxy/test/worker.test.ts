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

  it("restores aliased forbidden headers before forwarding upstream", async () => {
    let sentHeaders: Headers | undefined;
    const fetchMock = vi.fn(async (_url: any, init: any) => {
      sentHeaders = new Headers(init.headers);
      return new Response("ok", { status: 200 });
    });
    vi.stubGlobal("fetch", fetchMock);

    const req = new Request("https://w/p?url=" + encodeURIComponent("https://yt/api"), {
      method: "POST",
      headers: {
        "x-ytdown-origin": "https://www.youtube.com",
        "x-ytdown-referer": "https://www.youtube.com/",
        "x-goog-api-key": "k",
      },
      body: "{}",
    });
    await worker.fetch(req);

    expect(sentHeaders!.get("origin")).toBe("https://www.youtube.com");
    expect(sentHeaders!.get("referer")).toBe("https://www.youtube.com/");
    expect(sentHeaders!.get("x-ytdown-origin")).toBeNull();
    expect(sentHeaders!.get("x-goog-api-key")).toBe("k");
  });
});
