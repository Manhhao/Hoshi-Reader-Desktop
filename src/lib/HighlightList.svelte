<script lang="ts">
  import { Highlighter, Trash2 } from "@lucide/svelte";
  import { highlightColors, type BookHighlight } from "./highlights";
  import { relativeDate } from "./relativeDate";
  import type { BookInfo, TocItem } from "./types";

  let { highlights, bookInfo, toc, onJump, onDelete }: {
    highlights: BookHighlight[];
    bookInfo: BookInfo;
    toc: TocItem[];
    onJump: (highlight: BookHighlight) => void;
    onDelete: (highlight: BookHighlight) => void;
  } = $props();

  const sections = $derived.by(() => {
    const labels = new Map<number, string>();
    let label = "";
    for (const item of toc) {
      if (item.indentLevel === 0) label = item.label;
      if (!labels.has(item.spineIndex)) labels.set(item.spineIndex, label);
    }
    const groups = new Map<number, BookHighlight[]>();
    for (const highlight of highlights) {
      const character = Math.min(highlight.character, bookInfo.characterCount - 1);
      let spine = Object.values(bookInfo.chapterInfo).find((chapter) =>
        character >= chapter.currentTotal && character < chapter.currentTotal + chapter.chapterCount,
      )?.spineIndex ?? -1;
      while (spine > 0 && !labels.has(spine)) spine--;
      const group = groups.get(spine) ?? [];
      group.push(highlight);
      groups.set(spine, group);
    }
    return [...groups].sort(([a], [b]) => a - b).map(([spine, entries]) => ({
      spine,
      label: labels.get(spine) ?? "",
      highlights: entries.sort((a, b) => a.character - b.character),
    }));
  });

  function dateLabel(date: number) {
    const label = relativeDate(date);
    return label.charAt(0).toUpperCase() + label.slice(1);
  }
</script>

<div class="min-h-0 flex-1 overflow-y-auto">
  {#if highlights.length === 0}
    <div class="flex flex-col items-center gap-2 py-6 text-base-content/50">
      <Highlighter class="size-8" /><span class="text-sm">No Highlights</span>
    </div>
  {:else}
    {#each sections as section (section.spine)}
      <h3 class="px-4 py-2 text-sm font-semibold">{section.label}</h3>
      <ul>
        {#each section.highlights as highlight (highlight.id)}
          <li class="relative px-4 py-3 hover:bg-base-200">
            <button class="flex w-full flex-col gap-2 text-left" onclick={() => onJump(highlight)}>
              <span class="pr-7 text-sm"><span style:background-color={`rgba(${highlightColors[highlight.color].join(",")}, 0.35)`}>{(highlight.textFurigana ?? highlight.text).trim()}</span></span>
              <span class="flex w-full justify-between gap-2 text-xs text-base-content/60">
                <span>{dateLabel(highlight.createdAt)}</span><span>{highlight.character}</span>
              </span>
            </button>
            <button class="btn btn-ghost btn-xs btn-square absolute top-3 right-4 text-base-content/50 hover:text-error" title="Delete" onclick={() => onDelete(highlight)}><Trash2 class="size-3.5" /></button>
          </li>
        {/each}
      </ul>
    {/each}
  {/if}
</div>
