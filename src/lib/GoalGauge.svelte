<script lang="ts">
  import { ChevronRight } from "@lucide/svelte";
  import { statsConfig, saveStatsConfig, type StatisticsGoalMetric } from "./statsConfig.svelte";
  import { formatMinuteSecond, formatNumber, type StatisticsModel } from "./statsModel.svelte";

  let { model, editable = true }: { model: StatisticsModel; editable?: boolean } = $props();

  const metric = $derived(statsConfig.statisticsGoalMetric);
  const percent = $derived(
    Math.min(Math.round((model.metricValue(model.todaysReading) / model.goal) * 100), 100),
  );

  const headline = $derived(
    metric === "time"
      ? formatMinuteSecond(model.todaysReading.readingTime)
      : formatNumber(model.todaysReading.charactersRead),
  );

  const secondary = $derived(
    metric === "time"
      ? `${formatNumber(model.todaysReading.charactersRead)} characters`
      : `${Math.round(model.todaysReading.readingTime / 60)} minutes`,
  );

  const goalLabel = $derived(
    metric === "time"
      ? `/ ${statsConfig.statisticsDailyTimeGoal} minute goal`
      : `/ ${formatNumber(statsConfig.statisticsDailyCharacterGoal)} character goal`,
  );

  function setMetric(value: StatisticsGoalMetric) {
    if (statsConfig.statisticsGoalMetric === value) return;
    statsConfig.statisticsGoalMetric = value;
    saveStatsConfig();
  }

  let goalOpen = $state(false);
  let goalPicker = $state<HTMLDivElement>();

  $effect(() => {
    if (!goalOpen) return;
    const outside = (event: Event) => {
      if (!goalPicker?.contains(event.target as Node)) goalOpen = false;
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") goalOpen = false;
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape);
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", escape);
    };
  });
</script>

<div class="relative w-64 max-w-full shrink-0">
  <svg viewBox="0 0 124 64" class="w-full">
    <path
      d="M 2 62 A 60 60 0 0 1 122 62"
      fill="none"
      stroke="currentColor"
      stroke-width="4"
      stroke-linecap="round"
      class="text-base-content/10"
    />
    <path
      d="M 2 62 A 60 60 0 0 1 122 62"
      fill="none"
      stroke="currentColor"
      stroke-width="4"
      stroke-linecap="round"
      pathLength="100"
      stroke-dasharray="100"
      stroke-dashoffset={100 - Math.max(percent, 0.5)}
      class="statistics-gauge text-primary"
    />
  </svg>
  <div class="absolute inset-0 flex flex-col items-center justify-end px-6 pb-2">
    <span class="text-sm font-semibold">
      Today{#if percent >= 100}<span class="ml-1 text-primary">✓</span>{/if}
    </span>
    <span class="text-[2rem] leading-tight tabular-nums">{headline}</span>
    <span class="text-xs text-base-content/60 tabular-nums">{secondary}</span>

    {#if editable}
      <div
        bind:this={goalPicker}
        class="dropdown dropdown-center {goalOpen ? 'dropdown-open' : 'dropdown-close'}"
      >
        <button
          class="btn btn-ghost btn-xs font-normal text-base-content/60"
          onclick={() => (goalOpen = !goalOpen)}
        >
          {goalLabel}
          <ChevronRight class="size-3" />
        </button>
        <div
          class="dropdown-content z-30 mt-1 w-56 flex-col gap-3 rounded-box border border-base-300 bg-base-100 p-3 shadow-md {goalOpen ? 'flex' : 'hidden'}"
        >
          <div class="join w-full">
            <button
              class="btn join-item btn-sm flex-1 {metric === 'time' ? 'btn-active' : ''}"
              onclick={() => setMetric("time")}
            >
              Time
            </button>
            <button
              class="btn join-item btn-sm flex-1 {metric === 'characters' ? 'btn-active' : ''}"
              onclick={() => setMetric("characters")}
            >
              Characters
            </button>
          </div>
          {#if metric === "time"}
            <label class="input input-sm w-full">
              <input
                type="number"
                class="min-w-0 flex-1 tabular-nums"
                min="1"
                step="1"
                required
                value={statsConfig.statisticsDailyTimeGoal}
                onchange={(e) => {
                  if (!e.currentTarget.reportValidity()) return;
                  statsConfig.statisticsDailyTimeGoal = e.currentTarget.valueAsNumber;
                  saveStatsConfig();
                }}
              />
              <span class="text-base-content/60">min</span>
            </label>
          {:else}
            <input
              type="number"
              class="input input-sm w-full tabular-nums"
              min="1"
              step="1"
              required
              value={statsConfig.statisticsDailyCharacterGoal}
              onchange={(e) => {
                if (!e.currentTarget.reportValidity()) return;
                statsConfig.statisticsDailyCharacterGoal = e.currentTarget.valueAsNumber;
                saveStatsConfig();
              }}
            />
          {/if}
        </div>
      </div>
    {:else}
      <span class="flex h-6 items-center text-xs text-base-content/60">{goalLabel}</span>
    {/if}
  </div>
</div>
