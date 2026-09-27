<script lang="ts">
  import GoalGauge from "./GoalGauge.svelte";
  import { statsConfig } from "./statsConfig.svelte";
  import {
    addDays,
    dayKey,
    firstWeekday,
    formatNumber,
    formatUnits,
    keyToDate,
    startOfWeek,
    type StatisticsModel,
  } from "./statsModel.svelte";

  let { model }: { model: StatisticsModel } = $props();

  const metric = $derived(statsConfig.statisticsGoalMetric);
  const streak = $derived(model.streaks);
  const best = $derived(
    model.days.reduce<(typeof model.days)[number] | null>(
      (top, day) => (top === null || model.metricValue(day) > model.metricValue(top) ? day : top),
      null,
    ),
  );
  const bestValue = $derived(
    best === null
      ? "—"
      : metric === "time"
        ? formatUnits(best.readingTime)
        : formatNumber(best.charactersRead),
  );

  const dayMonth = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
  const dayMonthYear = new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
  const monthOnly = new Intl.DateTimeFormat(undefined, { month: "narrow" });
  const weekdayNarrow = new Intl.DateTimeFormat(undefined, { weekday: "narrow" });

  function streakDate(key: string): string {
    const date = keyToDate(key);
    const sameYear = date.getFullYear() === keyToDate(model.today).getFullYear();
    return (sameYear ? dayMonth : dayMonthYear).format(date);
  }

  const weeks = $derived.by(() => {
    const end = startOfWeek(keyToDate(model.today));
    let cursor = startOfWeek(keyToDate(model.firstDay ?? model.today));
    const columns: { key: string; month: string | null; days: string[] }[] = [];
    while (cursor <= end) {
      columns.push({
        key: dayKey(cursor),
        month: columns.length === 0 || cursor.getDate() <= 7 ? monthOnly.format(cursor) : null,
        days: Array.from({ length: 7 }, (_, i) => dayKey(addDays(cursor, i))),
      });
      cursor = addDays(cursor, 7);
    }
    return columns;
  });

  const cells = $derived(weeks.flatMap((week) => week.days));

  const weekdayLabels = Array.from({ length: 7 }, (_, offset) =>
    offset % 2 === 0 ? weekdayNarrow.format(addDays(new Date(2024, 0, 7), firstWeekday() + offset)) : "",
  );

  const columns = $derived(`repeat(${weeks.length}, 0.625rem)`);

  let hoveredDay = $state<{ key: string; x: number; y: number; below: boolean } | null>(null);

  function showDayTooltip(event: PointerEvent, key: string) {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const below = rect.top < 88;
    hoveredDay = {
      key,
      x: Math.max(104, Math.min(window.innerWidth - 104, rect.left + rect.width / 2)),
      y: below ? rect.bottom + 8 : rect.top - 8,
      below,
    };
  }
</script>

<svelte:window
  onscrollcapture={() => (hoveredDay = null)}
  onresize={() => (hoveredDay = null)}
/>

<div class="card border border-base-300 bg-base-100">
  <div class="card-body gap-4 p-4">
    <div class="flex flex-wrap items-center justify-center gap-x-8 gap-y-5">
      <GoalGauge {model} />

      <div
        class="grid min-w-[19rem] flex-1 grid-cols-2 rounded-box border border-base-300 [&>*]:border-base-300 [&>*:nth-child(odd)]:border-e [&>*:nth-child(-n+2)]:border-b"
      >
        <div class="flex flex-col px-4 py-3">
          <span class="text-xs text-base-content/60">Current Streak</span>
          <span class="text-lg font-medium tabular-nums">{streak.current.count} days</span>
          {#if streak.current.count > 0}
            <span class="text-xs text-base-content/60 tabular-nums">
              since {streakDate(streak.current.from)}
            </span>
          {/if}
        </div>
        <div class="flex flex-col px-4 py-3">
          <span class="text-xs text-base-content/60">Longest Streak</span>
          <span class="text-lg font-medium tabular-nums">{streak.longest.count} days</span>
          {#if streak.longest.count > 0}
            <span class="text-xs text-base-content/60 tabular-nums">
              {streak.longest.from === streak.longest.to
                ? streakDate(streak.longest.from)
                : `${streakDate(streak.longest.from)} – ${streakDate(streak.longest.to)}`}
            </span>
          {/if}
        </div>
        <div class="flex flex-col px-4 py-3">
          <span class="text-xs text-base-content/60">Days Met</span>
          <span class="text-lg font-medium tabular-nums">{model.goalDates.length} days</span>
          <span class="text-xs text-base-content/60 tabular-nums">
            of {model.days.length} days read
          </span>
        </div>
        <div class="flex flex-col px-4 py-3">
          <span class="text-xs text-base-content/60">Best Day</span>
          <span class="text-lg font-medium tabular-nums">{bestValue}</span>
          {#if best}
            <span class="text-xs text-base-content/60 tabular-nums">{streakDate(best.dateKey)}</span>
          {/if}
        </div>
      </div>
    </div>

    <div class="divider my-0"></div>

    <div class="flex items-start gap-2">
      <div class="grid shrink-0 gap-[3px] pt-[17px]" style="grid-template-rows: repeat(7, 0.625rem)">
        {#each weekdayLabels as label, i (i)}
          <span class="text-[10px] leading-[0.625rem] text-base-content/60">{label}</span>
        {/each}
      </div>
      <div class="min-w-0 flex-1 overflow-x-auto pb-1">
        <div class="mb-[3px] grid w-max gap-[3px]" style="grid-template-columns: {columns}">
          {#each weeks as week (week.key)}
            <span class="h-[14px] text-[10px] leading-[14px] whitespace-nowrap text-base-content/60">
              {week.month ?? ""}
            </span>
          {/each}
        </div>
        <div
          class="grid w-max grid-flow-col gap-[3px]"
          style="grid-template-columns: {columns}; grid-template-rows: repeat(7, 0.625rem)"
        >
          {#each cells as key (key)}
            {#if key > model.today}
              <span></span>
            {:else}
              {@const day = model.day(key)}
              <span
                class="size-2.5 rounded-[2.5px] {model.goalMet(day)
                  ? 'bg-primary'
                  : day
                    ? 'border border-primary'
                    : 'bg-base-content/10'} {key === model.today
                  ? 'outline outline-offset-1 outline-base-content'
                  : ''}"
                onpointerenter={(event) => showDayTooltip(event, key)}
                onpointerleave={() => (hoveredDay = null)}
              ></span>
            {/if}
          {/each}
        </div>
      </div>
    </div>

    <div class="flex items-center gap-3.5 text-xs text-base-content/60">
      <span class="flex items-center gap-1.5"><span class="size-2.5 rounded-[2.5px] border border-primary"></span>Read</span>
      <span class="flex items-center gap-1.5"><span class="size-2.5 rounded-[2.5px] bg-primary"></span>Goal met</span>
    </div>
  </div>
</div>

{#if hoveredDay}
  {@const day = model.day(hoveredDay.key)}
  <div
    class="pointer-events-none fixed z-50 flex w-48 flex-col gap-1 rounded-field bg-neutral px-3 py-2 text-xs text-neutral-content shadow-md"
    style:left="{hoveredDay.x}px"
    style:top="{hoveredDay.y}px"
    style:transform={hoveredDay.below ? "translateX(-50%)" : "translate(-50%, -100%)"}
  >
    <span class="font-semibold">{dayMonthYear.format(keyToDate(hoveredDay.key))}</span>
    <span>{formatUnits(day?.readingTime ?? 0)}, {day?.charactersRead ?? 0} characters</span>
  </div>
{/if}
