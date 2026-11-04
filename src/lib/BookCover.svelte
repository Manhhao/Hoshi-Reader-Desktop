<script lang="ts">
  import type { Snippet } from "svelte";
  import { shellConfig } from "./shellConfig.svelte";
  import { resolvedScheme } from "./theme.svelte";

  let {
    title,
    author,
    src,
    bunko = false,
    frontClass = "",
    children,
  }: {
    title: string;
    author?: string | null;
    src: string | null;
    bunko?: boolean;
    frontClass?: string;
    children?: Snippet;
  } = $props();

  const uid = $props.id();
  const aspectRatio = 0.709;
  const radius = 2;

  let width = $state(0);
  let scale = $state(window.devicePixelRatio || 1);

  function color(hue: number, saturation: number, brightness: number) {
    const channel = (n: number) => {
      const k = (n + hue * 6) % 6;
      return Math.round((brightness - brightness * saturation * Math.max(0, Math.min(k, 4 - k, 1))) * 255);
    };
    return `rgb(${channel(5)}, ${channel(3)}, ${channel(1)})`;
  }

  const gradient = $derived.by(() => {
    let hash = 0xcbf29ce484222325n;
    for (const byte of new TextEncoder().encode(title)) {
      hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
    }
    const hue = Number(hash % 3600n) / 3600;
    return `linear-gradient(to bottom right, ${color(hue, 0.42, 0.6)}, ${color((hue + 0.06) % 1, 0.58, 0.6 * 0.61)})`;
  });

  function depthOf(coverWidth: number) {
    return Math.max(2, Math.round((coverWidth * 0.02 * scale) / 2) * 2) / scale;
  }

  const coverWidth = $derived(width - depthOf(width));
  const depth = $derived(depthOf(coverWidth));
  const coverHeight = $derived(coverWidth / aspectRatio);
  const minX = $derived(width - coverWidth);
  const shadow = $derived(Math.min(1, coverWidth / 120));

  type Point = [number, number];

  function pageBlock(minX: number, maxX: number, minY: number, maxY: number) {
    const left = Math.round(minX * scale) / scale;
    const bottom = Math.round(maxY * scale) / scale;
    const pixel = 1 / scale;
    const corners = (offset: number) => ({
      topLeft: [left - offset + radius, minY + offset + radius] as Point,
      bottomLeft: [left - offset + radius, bottom + offset - radius] as Point,
      bottomRight: [maxX - offset - radius, bottom + offset - radius] as Point,
    });
    const point = (center: Point, degrees: number): Point => {
      const angle = (degrees * Math.PI) / 180;
      return [center[0] + radius * Math.cos(angle), center[1] + radius * Math.sin(angle)];
    };
    const front = corners(0);
    const back = corners(depth);

    let path = "";
    const move = ([x, y]: Point) => (path += `M${x} ${y}`);
    const line = ([x, y]: Point) => (path += `L${x} ${y}`);
    const arc = (center: Point, from: number, to: number) => {
      line(point(center, from));
      const [x, y] = point(center, to);
      path += `A${radius} ${radius} 0 0 ${to > from ? 1 : 0} ${x} ${y}`;
    };
    const take = () => {
      const result = path;
      path = "";
      return result;
    };
    const pageLine = (offset: number) => {
      const corner = corners(offset);
      move(point(corner.topLeft, 225));
      arc(corner.topLeft, 225, 180);
      arc(corner.bottomLeft, 180, 90);
      arc(corner.bottomRight, 90, 45);
    };

    move(point(front.topLeft, 225));
    line([maxX, minY]);
    arc(front.bottomRight, 0, 45);
    arc(back.bottomRight, 45, 90);
    arc(back.bottomLeft, 90, 180);
    arc(back.topLeft, 180, 225);
    path += "Z";
    const silhouette = take();

    move(point(front.topLeft, 225));
    arc(back.topLeft, 225, 180);
    arc(back.bottomLeft, 180, 135);
    arc(front.bottomLeft, 135, 180);
    arc(front.topLeft, 180, 225);
    path += "Z";
    const side = take();

    move(point(front.bottomLeft, 135));
    arc(back.bottomLeft, 135, 90);
    arc(back.bottomRight, 90, 45);
    arc(front.bottomRight, 45, 90);
    arc(front.bottomLeft, 90, 135);
    path += "Z";
    const bottomFace = take();

    for (let offset = 2 * pixel; offset < depth - pixel; offset += 2 * pixel) {
      pageLine(offset - pixel / 2);
    }
    const lines = take();

    const edge = depth - pixel / 2;
    pageLine(edge);
    const corner = corners(edge);
    for (const [from, to, degrees] of [
      [front.topLeft, corner.topLeft, 225],
      [front.bottomLeft, corner.bottomLeft, 135],
      [front.bottomRight, corner.bottomRight, 45],
    ] as const) {
      move(point(from, degrees));
      line(point(to, degrees));
    }
    const outline = take();

    return { silhouette, side, bottom: bottomFace, lines, outline, pixel };
  }

  const block = $derived(width > 0 ? pageBlock(minX, width, 0, coverHeight) : null);
  const dark = $derived(resolvedScheme() === "dark");
  const gray = (light: number, darkValue: number) => {
    const value = Math.round((dark ? darkValue : light) * 255);
    return `rgb(${value}, ${value}, ${value})`;
  };
</script>

<svelte:window onresize={() => (scale = window.devicePixelRatio || 1)} />

{#snippet image()}
  <div class="h-full w-full overflow-hidden [container-type:inline-size]">
    {#if src && shellConfig.bookshelfCoverMode !== "Hide"}
      <img {src} alt="" class="h-full w-full object-cover" style:filter={shellConfig.bookshelfCoverMode === "Blur" ? "blur(5cqw)" : ""} />
    {:else}
      <div class="flex h-full flex-col gap-[8cqw] px-[8cqw] pb-[8cqw] pt-[13.333cqw] text-center text-white" style:background={gradient}>
        <span class="line-clamp-5 text-[10cqw] font-semibold leading-snug">{title}</span>
        {#if author}
          <span class="mt-auto line-clamp-2 text-[8cqw] font-medium leading-snug text-white/75">{author}</span>
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

{#if bunko}
  <div
    class="relative w-full {width ? '' : 'aspect-[0.709]'}"
    style:height={width ? `${coverHeight + depth}px` : undefined}
    bind:clientWidth={width}
  >
    {#if block}
      <svg class="pointer-events-none absolute inset-0 overflow-visible" width={width} height={coverHeight + depth} aria-hidden="true">
        <defs>
          <filter id="{uid}-shadow" x="-50%" y="-50%" width="200%" height="200%">
            <feDropShadow dx="0" dy={4 * shadow} stdDeviation={4 * shadow} flood-color="#000" flood-opacity="0.4" />
          </filter>
          <linearGradient id="{uid}-bottom" gradientUnits="userSpaceOnUse" x1={minX} y1="0" x2={width} y2="0">
            <stop offset="0.94" stop-color={gray(0.88, 0.46)} />
            <stop offset="1" stop-color={gray(0.72, 0.32)} />
          </linearGradient>
        </defs>
        <path d={block.silhouette} fill={gray(0.8, 0.26)} filter="url(#{uid}-shadow)" />
        <path d={block.side} fill={gray(0.94, 0.6)} />
        <path d={block.bottom} fill="url(#{uid}-bottom)" />
        <path d={block.lines} fill="none" stroke="rgba(0, 0, 0, 0.16)" stroke-width={block.pixel} />
        <path d={block.outline} fill="none" stroke={gray(0.75, 0.27)} stroke-width={block.pixel} />
      </svg>
    {/if}
    <div
      class="absolute right-0 top-0 overflow-hidden rounded-[2px] {frontClass}"
      style:width={width ? `${coverWidth}px` : "100%"}
      style:height={width ? `${coverHeight}px` : "100%"}
    >
      {@render image()}
      <div
        class="pointer-events-none absolute inset-0"
        style:background="linear-gradient(to right, transparent 82%, rgba(0, 0, 0, 0.02) 90%, rgba(0, 0, 0, 0.06) 95%, rgba(0, 0, 0, 0.12) 98%, rgba(0, 0, 0, 0.2) 100%)"
      ></div>
      <div
        class="pointer-events-none absolute inset-0"
        style:background="linear-gradient(to bottom right, rgba(255, 255, 255, 0.16), transparent, transparent, rgba(255, 255, 255, 0.04))"
      ></div>
      <div class="pointer-events-none absolute inset-0 rounded-[2px] border-[0.5px] border-base-content/10"></div>
    </div>
    {#if children}
      <div
        class="pointer-events-none absolute right-0 top-0"
        style:width={width ? `${coverWidth}px` : "100%"}
        style:height={width ? `${coverHeight}px` : "100%"}
      >
        {@render children()}
      </div>
    {/if}
  </div>
{:else}
  {@render image()}
{/if}
