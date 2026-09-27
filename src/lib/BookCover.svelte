<script lang="ts">
  import { shellConfig } from "./shellConfig.svelte";

  let { title, author, src }: { title: string; author?: string | null; src: string | null } = $props();

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
</script>

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
