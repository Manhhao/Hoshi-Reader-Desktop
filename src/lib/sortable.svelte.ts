type SortableOptions = {
  commit: () => void;
  list?: () => unknown[];
  threshold?: number;
  move?: (from: number, to: number) => void;
};

export class Sortable {
  index = $state<number | null>(null);
  dragged = $state(false);

  #commit: () => void;
  #list: (() => unknown[]) | undefined;
  #threshold: number;
  #reorder: ((from: number, to: number) => void) | null;
  #container: HTMLElement | null = null;
  #origin: { index: number; x: number; y: number } | null = null;

  constructor(options: SortableOptions) {
    this.#commit = options.commit;
    this.#list = options.list;
    this.#threshold = options.threshold ?? 0;
    this.#reorder = options.move ?? null;
  }

  container = (node: HTMLElement) => {
    this.#container = node;
    return {
      destroy: () => {
        if (this.#container === node) this.#container = null;
      },
    };
  };

  start = (index: number, e: PointerEvent) => {
    this.dragged = false;
    this.#origin = { index, x: e.clientX, y: e.clientY };
    if (this.#threshold === 0) {
      e.preventDefault();
      this.index = index;
      this.dragged = true;
      document.documentElement.classList.add("dragging");
    }

    const move = (ev: PointerEvent) => this.#move(ev);
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      this.#origin = null;
      if (this.index === null) return;
      document.documentElement.classList.remove("dragging");
      this.index = null;
      this.#commit();
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  };

  #move(e: PointerEvent) {
    if (this.index === null) {
      if (!this.#origin) return;
      if (Math.hypot(e.clientX - this.#origin.x, e.clientY - this.#origin.y) < this.#threshold) return;
      this.index = this.#origin.index;
      this.dragged = true;
      document.documentElement.classList.add("dragging");
    }
    if (!this.#container) return;
    const rows = [...this.#container.children] as HTMLElement[];
    if (rows.length === 0) return;

    let to = -1;
    let nearest = Infinity;
    rows.forEach((row, index) => {
      const r = row.getBoundingClientRect();
      const dx = e.clientX - (r.left + r.right) / 2;
      const dy = e.clientY - (r.top + r.bottom) / 2;
      const distance = dx * dx + dy * dy;
      if (distance < nearest) {
        nearest = distance;
        to = index;
      }
    });
    if (to === -1 || to === this.index) return;

    if (this.#reorder) {
      this.#reorder(this.index, to);
    } else {
      const list = this.#list!();
      const [moved] = list.splice(this.index, 1);
      list.splice(to, 0, moved);
    }
    this.index = to;
  }
}
