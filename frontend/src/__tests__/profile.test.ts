import { describe, expect, it } from "bun:test";
import { formatBytes, rates, report, Samples } from "@domain/profile";
import type { SessionStats } from "@domain/types";

const stats = (over: Partial<SessionStats> = {}): SessionStats => ({
  uptime_ms: 0,
  output_bytes: 0,
  output_reads: 0,
  parse_us: 0,
  frames_built: 0,
  frames_sent: 0,
  frames_skipped: 0,
  rows_sent: 0,
  build_us: 0,
  build_max_us: 0,
  input_bytes: 0,
  input_writes: 0,
  input_pending: 0,
  history: 0,
  memory_bytes: null,
  ...over,
});

describe("Samples", () => {
  it("summarises the most recent values", () => {
    const samples = new Samples(4);
    expect(samples.summary()).toEqual({ avg: 0, p95: 0, max: 0 });
    for (const v of [100, 1, 2, 3, 4]) samples.add(v);
    // 100 was pushed out by the newer four.
    expect(samples.summary()).toEqual({ avg: 2.5, p95: 4, max: 4 });
  });

  it("takes the 95th percentile", () => {
    const samples = new Samples(100);
    for (let i = 1; i <= 100; i++) samples.add(i);
    expect(samples.summary().p95).toBe(95);
  });
});

describe("rates", () => {
  it("turns two snapshots into per-second numbers", () => {
    const before = stats({ uptime_ms: 1000, output_bytes: 100, frames_sent: 10, frames_built: 10, build_us: 1000 });
    const after = stats({ uptime_ms: 3000, output_bytes: 2100, frames_sent: 30, frames_built: 30, build_us: 5000, parse_us: 4000 });
    const r = rates(before, after);
    expect(r.outputBytes).toBe(1000);
    expect(r.framesSent).toBe(10);
    expect(r.parseMs).toBe(2);
    // 4 ms over 20 frames.
    expect(r.buildMs).toBeCloseTo(0.2);
  });
});

describe("report", () => {
  it("formats sizes and lists every section", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KiB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MiB");
    const zero = { avg: 0, p95: 0, max: 0 };
    const ui = {
      paint: zero,
      latency: zero,
      latencySamples: 0,
      paintsPerSecond: 0,
      framesPerSecond: 0,
      domNodes: 1,
      cards: 0,
      jsHeapBytes: null,
    };
    const lines = report(ui, stats({ memory_bytes: 1024 * 1024 }), rates(stats(), stats({ uptime_ms: 1000 })));
    expect(lines.map((l) => l.split(" ")[0])).toEqual(["paint", "latency", "ui", "output", "frames", "memory", "totals"]);
    expect(lines.find((l) => l.startsWith("memory"))).toContain("1.0 MiB");
  });
});
