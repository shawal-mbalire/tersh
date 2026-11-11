// The Angular shell: lays out the terminal's elements and hands them, with
// the injected backend, to the framework-free terminal (adapters/dom/boot).
// Nothing on the hot path (keys, frames, painting) goes through Angular: the
// app runs zoneless and this component never re-renders.

import { ChangeDetectionStrategy, Component, ElementRef, ViewChild, ViewEncapsulation, inject, type AfterViewInit } from "@angular/core";
import { showFatal } from "@adapters/dom/boot";
import { BACKEND, TERMINAL_BOOT } from "./tokens";

@Component({
  selector: "tersh-root",
  encapsulation: ViewEncapsulation.None,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div id="drag" data-tauri-drag-region></div>
    <div id="terminal" #terminal>
      <canvas id="grid" #grid></canvas>
      <div id="selection" #selection></div>
      <div id="cursor" #cursor></div>
      <div id="cards" #cards></div>
      <textarea
        id="input"
        #input
        autocapitalize="off"
        autocomplete="off"
        autocorrect="off"
        spellcheck="false"
        aria-label="Terminal input"
      ></textarea>
    </div>
    <div id="toasts" #toasts></div>
  `,
})
export class TerminalComponent implements AfterViewInit {
  private readonly backend = inject(BACKEND);
  private readonly boot = inject(TERMINAL_BOOT);

  // Decorator queries rather than signal queries (`viewChild`): the elements
  // are static, and decorators also work when tests compile the component
  // just in time, which signal queries don't without Angular's build step.
  @ViewChild("terminal", { static: true }) private terminal!: ElementRef<HTMLElement>;
  @ViewChild("grid", { static: true }) private grid!: ElementRef<HTMLCanvasElement>;
  @ViewChild("selection", { static: true }) private selection!: ElementRef<HTMLElement>;
  @ViewChild("cursor", { static: true }) private cursor!: ElementRef<HTMLElement>;
  @ViewChild("cards", { static: true }) private cards!: ElementRef<HTMLElement>;
  @ViewChild("input", { static: true }) private input!: ElementRef<HTMLTextAreaElement>;
  @ViewChild("toasts", { static: true }) private toasts!: ElementRef<HTMLElement>;

  ngAfterViewInit() {
    this.boot(this.backend, {
      terminal: this.terminal.nativeElement,
      canvas: this.grid.nativeElement,
      selection: this.selection.nativeElement,
      cursor: this.cursor.nativeElement,
      cards: this.cards.nativeElement,
      input: this.input.nativeElement,
      toasts: this.toasts.nativeElement,
    }).catch((e) => showFatal(this.backend, e));
  }
}
