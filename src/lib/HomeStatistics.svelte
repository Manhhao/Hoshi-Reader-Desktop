<script lang="ts">
  import { ArrowUpRight } from "@lucide/svelte";
  import GoalGauge from "./GoalGauge.svelte";
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

  let { model, onOpen }: { model: StatisticsModel; onOpen: () => void } = $props();

  const monthName = new Intl.DateTimeFormat(undefined, { month: "short" });
  const weekdayName = new Intl.DateTimeFormat(undefined, { weekday: "narrow" });
  const weekdayShort = new Intl.DateTimeFormat(undefined, { weekday: "short" });
  const recentDays = $derived(Array.from({ length: 7 }, (_, index) => {
    const key = dayKey(addDays(keyToDate(model.today), index - 6));
    return model.day(key) ?? { dateKey: key, readingTime: 0, charactersRead: 0 };
  }));
  const recentTime = $derived(recentDays.reduce((total, day) => total + day.readingTime, 0));
  const recentAverage = $derived(recentTime / 7);
  const recentCharacters = $derived(recentDays.reduce((total, day) => total + day.charactersRead, 0));
  const chartMax = $derived(Math.max(7200, Math.ceil(Math.max(...recentDays.map((day) => day.readingTime)) / 3600) * 3600));
  const streak = $derived(model.streaks.current.count);
  const first = $derived(addDays(startOfWeek(keyToDate(model.today)), -77));
  const weeks = $derived(Array.from({ length: 12 }, (_, week) => {
    const start = addDays(first, week * 7);
    return {
      key: dayKey(start),
      month: week === 0 || start.getDate() <= 7 ? monthName.format(start) : "",
      days: Array.from({ length: 7 }, (_, day) => dayKey(addDays(start, day))),
    };
  }));
  const weekdayLabels = Array.from({ length: 7 }, (_, index) => index % 2 === 0
    ? weekdayName.format(addDays(new Date(2024, 0, 7), firstWeekday() + index))
    : "");
</script>

<section class="@container/activity rounded-lg border border-base-300 bg-base-200/20 p-4">
  <header class="mb-5 flex flex-wrap items-center justify-between gap-3">
    <h2 class="text-sm font-medium">Activity</h2>
    <button class="btn btn-ghost btn-xs gap-1 text-base-content/60" onclick={onOpen}>
      View statistics <ArrowUpRight class="size-3.5" />
    </button>
  </header>
  <div class="grid items-start gap-6 @3xl/activity:grid-cols-[16rem_minmax(0,1fr)] @5xl/activity:grid-cols-[18rem_minmax(0,1fr)_15rem]">
    <div class="flex flex-col items-center gap-2 @5xl/activity:border-r @5xl/activity:border-base-300 @5xl/activity:pr-5">
      <GoalGauge {model} editable={false} />
      <div class="flex flex-col items-center gap-1 text-xs tabular-nums">
        <span class="text-base-content/60">Current streak</span>
        <span class="text-sm font-medium">{streak} {streak === 1 ? "day" : "days"}</span>
      </div>
    </div>

    <div class="flex min-w-0 flex-col gap-3 self-stretch @3xl/activity:col-start-2 @3xl/activity:row-span-2 @3xl/activity:row-start-1 @5xl/activity:row-span-1">
      <div class="flex flex-wrap items-end justify-between gap-2">
        <div class="flex flex-col gap-0.5">
          <h3 class="text-xs font-medium">Last 7 days</h3>
          <span class="text-lg font-medium leading-snug tabular-nums">{formatUnits(recentTime)}</span>
          <span class="text-xs text-base-content/60 tabular-nums">{formatNumber(recentCharacters)} characters</span>
        </div>
        <span class="text-[11px] text-base-content/60 tabular-nums">{formatUnits(recentAverage)} average</span>
      </div>
      <div class="flex min-h-32 flex-1 gap-2">
        <div class="flex min-w-0 flex-1 flex-col">
          <div class="relative min-h-28 flex-1">
            <div class="pointer-events-none absolute inset-0 flex flex-col justify-between">
              {#each [0, 1, 2, 3] as line (line)}
                <div class="border-t border-base-content/10"></div>
              {/each}
            </div>
            {#if recentAverage > 0}
              <div
                class="pointer-events-none absolute inset-x-0 border-t border-dashed border-primary"
                style:bottom="{recentAverage / chartMax * 100}%"
              >
                <span class="absolute left-full -translate-y-1/2 pl-1 text-[10px] whitespace-nowrap text-primary">
                  avg
                </span>
              </div>
            {/if}
            <div class="absolute inset-0 grid grid-cols-7 items-end">
              {#each recentDays as day (day.dateKey)}
                <div class="flex h-full items-end justify-center">
                  <span
                    class="statistics-bar w-1/2 rounded-t-sm bg-primary"
                    style:height="{day.readingTime / chartMax * 100}%"
                  ></span>
                </div>
              {/each}
            </div>
            {#if recentTime === 0}
              <span class="absolute inset-0 flex items-center justify-center text-xs text-base-content/50">No reading in the last 7 days</span>
            {/if}
          </div>
          <div class="mt-2 grid grid-cols-7 text-center text-[10px] text-base-content/60">
            {#each recentDays as day (day.dateKey)}
              <span class={day.dateKey === model.today ? "text-primary" : ""}>{day.dateKey === model.today ? "Today" : weekdayShort.format(keyToDate(day.dateKey))}</span>
            {/each}
          </div>
        </div>
        <div class="flex shrink-0 flex-col justify-between pb-6 text-[10px] text-base-content/60 tabular-nums">
          <span>{formatUnits(chartMax)}</span>
          <span>0</span>
        </div>
      </div>
    </div>

    <div class="flex min-w-0 flex-col gap-3 border-t border-base-300 pt-4 @3xl/activity:col-start-1 @3xl/activity:row-start-2 @5xl/activity:col-start-3 @5xl/activity:row-start-1 @5xl/activity:border-t-0 @5xl/activity:border-l @5xl/activity:pt-0 @5xl/activity:pl-5">
      <h3 class="text-xs font-medium">Last 12 weeks</h3>
      <div class="flex gap-2">
        <div class="grid shrink-0 grid-rows-7 gap-1 pt-4">
          {#each weekdayLabels as label, index (index)}
            <span class="h-3 text-[10px] leading-3 text-base-content/60">{label}</span>
          {/each}
        </div>
        <div class="grid grid-cols-[repeat(12,0.75rem)] gap-1">
          {#each weeks as week (week.key)}
            <div class="flex flex-col gap-1">
              <span class="h-3 text-[10px] leading-3 whitespace-nowrap text-base-content/60">{week.month}</span>
              {#each week.days as key (key)}
                {@const day = model.day(key)}
                <span
                  class="size-3 rounded-[2.5px] {key > model.today
                    ? ''
                    : model.goalMet(day)
                      ? 'bg-primary'
                      : day
                        ? 'border border-primary'
                        : 'bg-base-content/10'} {key === model.today ? 'outline outline-offset-1 outline-base-content' : ''}"
                ></span>
              {/each}
            </div>
          {/each}
        </div>
      </div>
      <div class="flex gap-3 text-[10px] text-base-content/60">
        <span class="flex items-center gap-1.5"><span class="size-2.5 rounded-[2.5px] border border-primary"></span>Read</span>
        <span class="flex items-center gap-1.5"><span class="size-2.5 rounded-[2.5px] bg-primary"></span>Goal met</span>
      </div>
    </div>
  </div>
</section>
