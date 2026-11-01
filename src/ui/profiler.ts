// The profiler: measures painting and input latency in the page, polls the
// backend's counters, and shows both in an overlay (the `toggle_profiler`
// action). Measuring is always on and costs two `performance.now()` calls per
// paint; polling and the overlay only run while it is visible.

import { type BackendRates, rates, report, Samples, type UiProfile } from "../domain/profile";
import type { SessionStats } from "../domain/types";

const POLL_MS = 1000;

/** Record `now` in `times`, keeping only the last second (so it stays small while hidden). */
function stamp(times: number[], now: number) {
  times.push(now);
  while (times[0] < now - 1000) times.shift();
}

export class Profiler {
  private paints = new Samples();
  private latencies = new Samples();
  /** When the oldest unanswered keystroke was sent. */
  private inputAt: number | null = null;
  private paintTimes: number[] = [];
  private frameTimes: number[] = [];
  private stats: SessionStats | null = null;
  private backend: BackendRates | null = null;
  private timer = 0;
  private el: HTMLElement | null = null;

  constructor(
    private readonly host: HTMLElement,
    private readonly fetchStats: () => Promise<SessionStats>,
    private readonly cardCount: () => number,
  ) {}

  get visible(): boolean {
    return this.el !== null;
  }

  /** Input was sent to the program. */
  input() {
    this.inputAt ??= performance.now();
  }

  /** A frame arrived from the backend. */
  frame() {
    stamp(this.frameTimes, performance.now());
  }

  /** A paint ran from `start` to `end` (ms, `performance.now()`). */
  painted(start: number, end: number) {
    this.paints.add(end - start);
    stamp(this.paintTimes, end);
    if (this.inputAt !== null) {
      this.latencies.add(end - this.inputAt);
      this.inputAt = null;
    }
  }

  /** What the page measured, for the overlay and `tersh.profile()`. */
  snapshot(): UiProfile {
    const now = performance.now();
    const recent = (times: number[]) => times.filter((t) => t >= now - 1000).length;
    const memory = (performance as Performance & { memory?: { usedJSHeapSize: number } }).memory;
    return {
      paint: this.paints.summary(),
      latency: this.latencies.summary(),
      latencySamples: this.latencies.count,
      paintsPerSecond: recent(this.paintTimes),
      framesPerSecond: recent(this.frameTimes),
      domNodes: document.getElementsByTagName("*").length,
      cards: this.cardCount(),
      jsHeapBytes: memory ? memory.usedJSHeapSize : null,
    };
  }

  toggle() {
    if (this.el) this.hide();
    else this.show();
  }

  show() {
    if (this.el) return;
    const el = document.createElement("pre");
    el.id = "profiler";
    this.host.append(el);
    this.el = el;
    const tick = async () => {
      try {
        const next = await this.fetchStats();
        if (this.stats) this.backend = rates(this.stats, next);
        this.stats = next;
      } catch {
        // No session yet, or it ended: show what the page measured.
      }
      this.render();
    };
    void tick();
    this.timer = window.setInterval(() => void tick(), POLL_MS);
  }

  hide() {
    window.clearInterval(this.timer);
    this.el?.remove();
    this.el = null;
    this.stats = null;
    this.backend = null;
  }

  private render() {
    if (!this.el) return;
    this.el.textContent = ["tersh profiler", ...report(this.snapshot(), this.stats, this.backend)].join("\n");
  }
}
