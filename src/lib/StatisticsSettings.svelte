<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { ask } from "@tauri-apps/plugin-dialog";
  import SettingRow from "./SettingRow.svelte";
  import SettingToggle from "./SettingToggle.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import {
    statsConfig,
    saveStatsConfig,
    type StatisticsAutostartMode,
  } from "./statsConfig.svelte";
  import { configureSync } from "./syncConfig.svelte";
  import type { BookStatistics } from "./types";

  const autostartModes: StatisticsAutostartMode[] = ["Off", "Page Turn", "On"];

  let archivedCount = $state(0);

  $effect(() => {
    loadArchivedCount();
    const unlisten = listen("sync://books-changed", loadArchivedCount);
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  async function loadArchivedCount() {
    const books = await invoke<BookStatistics[]>("load_all_statistics", {
      resetTime: Math.round(statsConfig.statisticsResetTime * 60),
    });
    archivedCount = books.filter((book) => book.isDeleted).length;
  }

  async function clearArchive() {
    if (!(await ask(`This will delete the statistics of ${archivedCount} deleted books.`, {
      title: "Clear Archive?",
      kind: "warning",
    }))) return;
    await invoke("clear_statistics_archive");
    await loadArchivedCount();
  }

  function resetTimeToInput(hours: number): string {
    const total = Math.round(hours * 60);
    const h = Math.floor(total / 60);
    const m = total % 60;
    return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
  }

  function parseResetTime(value: string): number {
    const [h, m] = value.split(":").map(Number);
    if (!Number.isFinite(h) || !Number.isFinite(m)) return statsConfig.statisticsResetTime;
    return h + m / 60;
  }
</script>

<section class="flex flex-col gap-3">
  <SettingRow compact label="Autostart">
    <div class="join">
      {#each autostartModes as mode (mode)}
        <button
          class="btn join-item btn-sm {statsConfig.statisticsAutostartMode === mode
            ? 'btn-active'
            : ''}"
          onclick={() => {
            if (statsConfig.statisticsAutostartMode === mode) return;
            statsConfig.statisticsAutostartMode = mode;
            saveStatsConfig();
          }}
        >
          {mode}
        </button>
      {/each}
    </div>
  </SettingRow>
  <SettingRow compact label="Reset Time">
    <input
      type="time"
      class="input input-sm w-32"
      value={resetTimeToInput(statsConfig.statisticsResetTime)}
      onchange={(e) => {
        statsConfig.statisticsResetTime = parseResetTime(e.currentTarget.value);
        saveStatsConfig();
        configureSync();
      }}
    />
  </SettingRow>
  <SettingToggle
    compact
    label="Hide Stats Overview on Home"
    bind:checked={statsConfig.statisticsHideOnHome}
    onchange={saveStatsConfig}
  />
</section>

{#if archivedCount > 0}
  <SettingsSection title="Archive" compact>
    <SettingRow compact label="Clear Archive">
      <button class="btn btn-error btn-sm" onclick={clearArchive}>Clear</button>
    </SettingRow>
    <p class="text-xs text-base-content/60">
      This will delete the statistics of {archivedCount} deleted books.
    </p>
  </SettingsSection>
{/if}
