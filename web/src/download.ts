export interface DownloadProgress {
  receivedBytes: number;
  totalBytes?: number;
  percent?: number;
}

export interface DownloadOptions {
  filename: string;
  /** Route the byte fetch through this proxy (same contract as resolve). */
  proxyUrl?: string;
  /** Extra headers the CDN URL requires. */
  headers?: [string, string][];
  onProgress?: (p: DownloadProgress) => void;
  fetchImpl?: typeof fetch;
}

/**
 * Download `url` to the user's disk. Streams via the File System Access API
 * when available (large files never fully buffer in memory); otherwise buffers
 * a Blob and triggers an `<a download>`.
 */
export async function downloadToDisk(url: string, opts: DownloadOptions): Promise<void> {
  const fetchImpl = opts.fetchImpl ?? fetch;
  const target = opts.proxyUrl ? `${opts.proxyUrl}?url=${encodeURIComponent(url)}` : url;
  const headers = new Headers(opts.headers ?? []);
  const res = await fetchImpl(target, { headers });
  if (!res.ok || !res.body) throw new Error(`download failed: HTTP ${res.status}`);

  const total = Number(res.headers.get("content-length")) || undefined;
  let received = 0;
  const report = (chunkLen: number) => {
    received += chunkLen;
    opts.onProgress?.({
      receivedBytes: received,
      totalBytes: total,
      percent: total ? (received / total) * 100 : undefined,
    });
  };

  const picker = (globalThis as any).showSaveFilePicker as
    | ((o?: unknown) => Promise<any>)
    | undefined;

  if (typeof picker === "function") {
    const handle = await picker({ suggestedName: opts.filename });
    const writable = await handle.createWritable();
    const reader = res.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      // value is guaranteed non-undefined when done === false, but TypeScript
      // types ReadableStreamReadResult<T> as { done: false; value: T } |
      // { done: true; value?: T }, so we cast to satisfy strict checks.
      await writable.write(value as Uint8Array);
      report((value as Uint8Array).byteLength);
    }
    await writable.close();
    return;
  }

  // Fallback: buffer into a Blob, then anchor-download.
  const reader = res.body.getReader();
  const parts: Uint8Array[] = [];
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value as Uint8Array);
    report((value as Uint8Array).byteLength);
  }
  const blob = new Blob(parts as BlobPart[]);
  const href = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = href;
  a.download = opts.filename;
  a.click();
  a.remove();
  URL.revokeObjectURL(href);
}
