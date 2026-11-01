// Profiling arithmetic: sample windows, percentiles, rates between two stats
// snapshots, and the text of the profiler overlay. No DOM.

import type { SessionStats } from "./types";

/** The most recent `capacity` samples (e.g. paint times in milliseconds). */
export class Samples {
  private values: number[] = [];
  private next = 0;

  constructor(private readonly capacity = 240) {}

  add(value: number) {
    if (this.values.length < this.capacity) this.values.push(value);
    else this.values[this.next] = value;
    this.next = (this.next + 1) % this.capacity;
  }

  get count(): number {
    return this.values.length;
  }

  summary(): { avg: number; p95: number; max: number } {
    const n = this.values.length;
    if (n === 0) return { avg: 0, p95: 0, max: 0 };
    const sorted = [...this.values].sort((a, b) => a - b);
    const sum = sorted.reduce((a, b) => a + b, 0);
    return { avg: sum / n, p95: sorted[Math.min(n - 1, Math.ceil(n * 0.95) - 1)], max: sorted[n - 1] };
  }
}

/** Per-second rates of the backend's counters between two snapshots. */
export interface BackendRates {
  outputBytes: number;
  parseMs: number;
  framesSent: number;
  framesSkipped: number;
  rowsSent: number;
  /** Average time to build a frame, in milliseconds. */
  buildMs: number;
  inputBytes: number;
}

export function rates(previous: SessionStats, current: SessionStats): BackendRates {
  const seconds = Math.max(0.001, (current.uptime_ms - previous.uptime_ms) / 1000);
  const per = (key: keyof SessionStats) => Math.max(0, Number(current[key] ?? 0) - Number(previous[key] ?? 0)) / seconds;
  const built = current.frames_built - previous.frames_built;
  return {
    outputBytes: per("output_bytes"),
    parseMs: per("parse_us") / 1000,
    framesSent: per("frames_sent"),
    framesSkipped: per("frames_skipped"),
    rowsSent: per("rows_sent"),
    buildMs: built > 0 ? (current.build_us - previous.build_us) / built / 1000 : 0,
    inputBytes: per("input_bytes"),
  };
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

const ms = (v: number) => (v < 10 ? v.toFixed(2) : v.toFixed(0));

/** What the UI side measured (see `Profiler` in `ui/profiler.ts`). */
export interface UiProfile {
  paint: { avg: number; p95: number; max: number };
  /** Keystroke written → next frame painted. */
  latency: { avg: number; p95: number; max: number };
  latencySamples: number;
  paintsPerSecond: number;
  framesPerSecond: number;
  domNodes: number;
  cards: number;
  jsHeapBytes: number | null;
}

/** Lines of the profiler overlay. */
export function report(ui: UiProfile, stats: SessionStats | null, backend: BackendRates | null): string[] {
  const lines = [
    `paint    ${ms(ui.paint.avg)} ms avg · ${ms(ui.paint.p95)} p95 · ${ms(ui.paint.max)} max · ${ui.paintsPerSecond.toFixed(0)}/s`,
    ui.latencySamples
      ? `latency  ${ms(ui.latency.avg)} ms avg · ${ms(ui.latency.p95)} p95 · ${ms(ui.latency.max)} max (key → screen)`
      : "latency  type something to measure (key → screen)",
    `ui       ${ui.framesPerSecond.toFixed(0)} frames/s received · ${ui.domNodes} DOM nodes · ${ui.cards} cards`,
  ];
  if (ui.jsHeapBytes !== null) lines.push(`js heap  ${formatBytes(ui.jsHeapBytes)}`);
  if (backend) {
    lines.push(
      `output   ${formatBytes(backend.outputBytes)}/s · parse ${ms(backend.parseMs)} ms/s`,
      `frames   ${backend.framesSent.toFixed(0)}/s sent · ${backend.framesSkipped.toFixed(0)}/s unchanged · ${backend.rowsSent.toFixed(0)} rows/s · build ${ms(backend.buildMs)} ms`,
    );
  }
  if (stats) {
    const memory = stats.memory_bytes === null ? "n/a" : formatBytes(stats.memory_bytes);
    lines.push(
      `memory   ${memory} resident · ${stats.history} lines of history · ${formatBytes(stats.input_pending)} input queued`,
      `totals   ${formatBytes(stats.output_bytes)} out · ${formatBytes(stats.input_bytes)} in · ${stats.frames_sent} frames · slowest build ${ms(stats.build_max_us / 1000)} ms`,
    );
  }
  return lines;
}
