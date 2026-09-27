<script lang="ts">
  let { src, active }: { src: string; active: boolean } = $props();

  let viewport = $state<HTMLDivElement>();
  let width = $state(0);
  let height = $state(0);
  let naturalWidth = $state(0);
  let naturalHeight = $state(0);
  let zoom = $state(1);
  let animateZoom = $state(false);
  let position = $state({ x: 0, y: 0 });
  let drag = $state<{ id: number; x: number; y: number; originX: number; originY: number } | null>(null);
  let gestureZoom: number | null = null;

  const fit = $derived(naturalWidth && naturalHeight ? Math.min(width / naturalWidth, height / naturalHeight) : 0);
  const imageWidth = $derived(naturalWidth ? naturalWidth * fit : width);
  const imageHeight = $derived(naturalHeight ? naturalHeight * fit : height);
  const limitX = $derived(Math.max(0, (imageWidth * zoom - width) / 2));
  const limitY = $derived(Math.max(0, (imageHeight * zoom - height) / 2));
  const offset = $derived({
    x: Math.max(-limitX, Math.min(position.x, limitX)),
    y: Math.max(-limitY, Math.min(position.y, limitY)),
  });

  export function getZoom() {
    return zoom;
  }

  export function reset(animate = true) {
    animateZoom = animate;
    zoom = 1;
    position = { x: 0, y: 0 };
    drag = null;
    gestureZoom = null;
  }

  export function setZoom(value: number, clientX?: number, clientY?: number, animate = true) {
    animateZoom = animate;
    const next = Math.max(1, Math.min(value, 5));
    if (next === 1) {
      zoom = 1;
      position = { x: 0, y: 0 };
      return;
    }
    const rect = viewport!.getBoundingClientRect();
    const x = clientX === undefined ? 0 : clientX - rect.left - width / 2;
    const y = clientY === undefined ? 0 : clientY - rect.top - height / 2;
    const ratio = next / zoom;
    position = { x: x - (x - offset.x) * ratio, y: y - (y - offset.y) * ratio };
    zoom = next;
  }

  function onDoubleClick(e: MouseEvent) {
    if (!active) return;
    e.preventDefault();
    setZoom(zoom === 1 ? 2.5 : 1, e.clientX, e.clientY);
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0 || zoom === 1) return;
    e.preventDefault();
    animateZoom = false;
    viewport!.setPointerCapture(e.pointerId);
    drag = { id: e.pointerId, x: e.clientX, y: e.clientY, originX: offset.x, originY: offset.y };
  }

  function onPointerMove(e: PointerEvent) {
    if (drag?.id !== e.pointerId) return;
    position = { x: drag.originX + e.clientX - drag.x, y: drag.originY + e.clientY - drag.y };
  }

  function onWheel(e: WheelEvent) {
    if (!active || (!e.ctrlKey && zoom === 1 && gestureZoom === null)) return;
    e.preventDefault();
    e.stopPropagation();
    if (gestureZoom !== null) return;
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? height : 1;
    if (e.ctrlKey) {
      setZoom(zoom * Math.exp(-e.deltaY * unit * 0.01), e.clientX, e.clientY, false);
    } else {
      animateZoom = false;
      position = { x: offset.x - e.deltaX * unit, y: offset.y - e.deltaY * unit };
    }
  }

  function onGestureStart(e: Event) {
    if (!active) return;
    e.preventDefault();
    gestureZoom = zoom;
  }

  function onGestureChange(event: Event) {
    if (gestureZoom === null) return;
    event.preventDefault();
    const e = event as Event & { scale: number; clientX: number; clientY: number };
    setZoom(gestureZoom * e.scale, e.clientX, e.clientY, false);
  }

  function onGestureEnd(e: Event) {
    if (gestureZoom === null) return;
    e.preventDefault();
    gestureZoom = null;
  }

  $effect(() => {
    if (!active) reset(false);
  });

  $effect(() => {
    const el = viewport;
    if (!el) return;
    el.addEventListener("dblclick", onDoubleClick);
    el.addEventListener("wheel", onWheel, { passive: false });
    el.addEventListener("gesturestart", onGestureStart, { passive: false });
    el.addEventListener("gesturechange", onGestureChange, { passive: false });
    el.addEventListener("gestureend", onGestureEnd, { passive: false });
    return () => {
      el.removeEventListener("dblclick", onDoubleClick);
      el.removeEventListener("wheel", onWheel);
      el.removeEventListener("gesturestart", onGestureStart);
      el.removeEventListener("gesturechange", onGestureChange);
      el.removeEventListener("gestureend", onGestureEnd);
    };
  });
</script>

<div class="h-full w-full px-12 pb-6">
  <div
    bind:this={viewport}
    bind:clientWidth={width}
    bind:clientHeight={height}
    class="flex h-full w-full touch-none select-none items-center justify-center overflow-hidden {zoom === 1 ? 'cursor-default' : drag ? 'cursor-grabbing' : 'cursor-grab'}"
    onpointerdown={onPointerDown}
    onmousedown={(e) => e.detail > 1 && e.preventDefault()}
    onpointermove={onPointerMove}
    onpointerup={() => (drag = null)}
    onpointercancel={() => (drag = null)}
    onlostpointercapture={() => (drag = null)}
  >
    <img
      {src}
      alt=""
      loading="lazy"
      draggable={false}
      class="pointer-events-none max-w-none shrink-0 select-none object-contain {animateZoom ? 'transition-transform duration-350 ease-in-out' : 'transition-none'}"
      style="width: {imageWidth}px; height: {imageHeight}px; transform: translate({offset.x}px, {offset.y}px) scale({zoom})"
      onload={(e) => {
        const image = e.currentTarget as HTMLImageElement;
        naturalWidth = image.naturalWidth;
        naturalHeight = image.naturalHeight;
      }}
    />
  </div>
</div>
