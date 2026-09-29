<script lang="ts">
  import { ChevronLeft, ChevronRight, TrendingDown, TrendingUp } from "@lucide/svelte";
  import {
    formatUnits,
    keyToDate,
    periods,
    type StatisticsModel,
  } from "./statsModel.svelte";

  let { model }: { model: StatisticsModel } = $props();

  const buckets = $derived(model.buckets(model.referenceDate));
  const average = $derived(model.averageReadingTime(model.referenceDate) ?? 0);
  const maxHours = $derived(
    Math.max(2, Math.ceil(Math.max(...buckets.map((day) => day.readingTime), 0) / 3600)),
  );
  const headlineValue = $derived(model.selectedDay?.readingTime ?? average);

  const index = $derived(
    model.referenceDates.findIndex(
      (date) => date.getTime() === model.referenceDate.getTime(),
    ),
  );

  const weekdayNarrow = new Intl.DateTimeFormat(undefined, { weekday: "narrow" });
  const monthNarrow = new Intl.DateTimeFormat(undefined, { month: "narrow" });
  const monthShortYear = new Intl.DateTimeFormat(undefined, { month: "short", year: "2-digit" });
  const dayOnly = new Intl.DateTimeFormat(undefined, { day: "numeric" });
  const dayMonth = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
  const weekdayDayMonth = new Intl.DateTimeFormat(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
  });
  const monthYear = new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" });

  function axisLabel(key: string, position: number): string {
    const date = keyToDate(key);
    switch (model.period) {
      case "Week":
        return weekdayNarrow.format(date);
      case "Month":
        return position % 7 === 0 ? dayOnly.format(date) : "";
      case "Year":
        return position % 2 === 0 ? monthNarrow.format(date) : "";
      default:
        return position % 3 === 0 ? monthShortYear.format(date) : "";
    }
  }

  const periodTitle = $derived.by(() => {
    if (model.period === "All") return "All Time";
    const start = model.periodStart(model.referenceDate)!;
    switch (model.period) {
      case "Week": {
        const end = buckets.at(-1)!;
        return `${dayMonth.format(start)} – ${dayMonth.format(keyToDate(end.dateKey))}`;
      }
      case "Month":
        return monthYear.format(start);
      default:
        return String(start.getFullYear());
    }
  });

  const headline = $derived.by(() => {
    const selected = model.selectedDay;
    if (!selected) return `${periodTitle} Average`;
    const date = keyToDate(selected.dateKey);
    return model.bucketUnit === "month" ? monthYear.format(date) : weekdayDayMonth.format(date);
  });

  const deltaLabel = $derived(
    model.period === "Week"
      ? "from previous week"
      : model.period === "Month"
        ? "from previous month"
        : "from previous year",
  );
</script>

<div class="card border border-base-300 bg-base-100">
  <div class="card-body gap-0 p-4">
    <div class="join w-full">
      {#each periods as period (period)}
        <button
          class="btn join-item btn-sm flex-1 {model.period === period ? 'btn-active' : ''}"
          onclick={() => model.setPeriod(period)}
        >
          {period}
        </button>
      {/each}
    </div>

    <div class="mt-3 flex items-start justify-between gap-3">
      <div class="flex min-w-0 flex-col">
        <span class="truncate text-base text-base-content/60">{headline}</span>
        <span class="text-[32px] leading-tight tabular-nums">{formatUnits(headlineValue)}</span>
      </div>
      <div class="flex shrink-0 items-center gap-2 pt-1">
        {#if model.readingTimeDelta !== null}
          <span class="flex items-center gap-1 text-xs text-base-content/60 tabular-nums">
            {#if model.readingTimeDelta >= 0}
              <TrendingUp class="size-3.5" />
            {:else}
              <TrendingDown class="size-3.5" />
            {/if}
            {Math.abs(Math.round(model.readingTimeDelta))}% {deltaLabel}
          </span>
        {/if}
        {#if model.period !== "All"}
          <div class="join">
            <button
              class="btn join-item btn-ghost btn-xs"
              disabled={index <= 0}
              onclick={() => model.setReferenceDate(model.referenceDates[index - 1])}
            >
              <ChevronLeft class="size-4" />
            </button>
            <button
              class="btn join-item btn-ghost btn-xs"
              disabled={index < 0 || index >= model.referenceDates.length - 1}
              onclick={() => model.setReferenceDate(model.referenceDates[index + 1])}
            >
              <ChevronRight class="size-4" />
            </button>
          </div>
        {/if}
      </div>
    </div>

    <div class="mt-3 flex gap-2">
      <div class="min-w-0 flex-1">
        <div class="relative h-28">
          <div class="pointer-events-none absolute inset-0 flex flex-col justify-between">
            {#each [0, 1, 2, 3, 4] as line (line)}
              <div class="border-t border-base-content/10"></div>
            {/each}
          </div>
          {#if average > 0}
            <div
              class="pointer-events-none absolute inset-x-0 border-t border-dashed border-primary"
              style="bottom: {Math.min((average / 3600 / maxHours) * 100, 100)}%"
            >
              <span class="absolute left-full -translate-y-1/2 pl-1 text-[10px] whitespace-nowrap text-primary">
                avg
              </span>
            </div>
          {/if}
          <div
            class="absolute inset-0 grid grid-flow-col items-end"
            style="grid-auto-columns: minmax(0, 1fr)"
          >
            {#each buckets as bucket (`${model.period}:${bucket.dateKey}`)}
              {@const active = model.selectedKey === null || model.selectedKey === bucket.dateKey}
              <button
                class="group flex h-full w-3/5 min-w-[3px] items-end justify-self-center"
                onclick={() => model.select(bucket.dateKey)}
              >
                <span
                  class="statistics-bar w-full rounded-t-[3px] {active
                    ? 'bg-primary'
                    : 'bg-base-content/20'} group-hover:bg-primary"
                  style="height: {bucket.readingTime > 0
                    ? Math.max((bucket.readingTime / 3600 / maxHours) * 100, 1.5)
                    : 0}%"
                ></span>
              </button>
            {/each}
          </div>
        </div>
        <div class="mt-1 grid grid-flow-col" style="grid-auto-columns: minmax(0, 1fr)">
          {#each buckets as bucket, i (bucket.dateKey)}
            <span
              class="truncate text-center text-[10px] text-base-content/60"
            >{axisLabel(bucket.dateKey, i)}</span>
          {/each}
        </div>
      </div>
      <div class="flex w-5 shrink-0 flex-col justify-between pb-[18px] text-[10px] text-base-content/60">
        <span>{maxHours}h</span>
        <span>0</span>
      </div>
    </div>
  </div>
</div>
