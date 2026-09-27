<script lang="ts">
  import { ChevronDown, ChevronRight } from "@lucide/svelte";
  import { colorAlpha, colorHex, sasayakiConfig, saveSasayakiConfig, withAlpha } from "./sasayakiConfig.svelte";
  import SettingGroup from "./SettingGroup.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingRow from "./SettingRow.svelte";
  import SettingToggle from "./SettingToggle.svelte";
  import SettingStepper from "./SettingStepper.svelte";

  let { compact = false }: { compact?: boolean } = $props();

  let showColors = $state(false);

  const themes = [
    ["Light Theme", "sasayakiTextColor", "sasayakiBackgroundColor"],
    ["Dark Theme", "sasayakiDarkTextColor", "sasayakiDarkBackgroundColor"],
  ] as const;
</script>

{#snippet settings()}
  <SettingToggle
    label="Auto-Scroll"
    {compact}
    bind:checked={sasayakiConfig.sasayakiAutoScroll}
    onchange={saveSasayakiConfig}
  />
  <SettingToggle
    label="Auto-Pause on Lookup"
    {compact}
    bind:checked={sasayakiConfig.sasayakiAutoPause}
    onchange={saveSasayakiConfig}
  />
  <SettingToggle
    label="Pause on Images"
    {compact}
    bind:checked={sasayakiConfig.sasayakiImagePause}
    onchange={saveSasayakiConfig}
  />
  {#if sasayakiConfig.sasayakiImagePause}
    <SettingStepper
      label="Pause Duration"
      {compact}
      bind:value={sasayakiConfig.sasayakiImagePauseDuration}
      suffix="s"
      min={1}
      max={30}
      onchange={saveSasayakiConfig}
    />
  {/if}
{/snippet}

{#snippet paragraphMode()}
  <SettingToggle
    label="Advance on Page Turn"
    {compact}
    bind:checked={sasayakiConfig.sasayakiPageAdvance}
    onchange={saveSasayakiConfig}
  />
{/snippet}

{#snippet controlBar()}
  <SettingToggle
    label="Show Control Bar"
    {compact}
    bind:checked={sasayakiConfig.sasayakiShowControlBar}
    onchange={saveSasayakiConfig}
  />
{/snippet}

{#snippet colors(text: (typeof themes)[number][1], background: (typeof themes)[number][2])}
  {#each [["Text Color", text], ["Background Color", background]] as const as [label, key] (key)}
    <SettingRow {label} {compact}>
      <div class="flex items-center gap-2">
        <input
          type="range"
          class="range range-xs text-primary {compact ? 'w-20' : 'w-24'}"
          min="0"
          max="1"
          step="0.05"
          value={colorAlpha(sasayakiConfig[key])}
          oninput={(e) => {
            sasayakiConfig[key] = withAlpha(sasayakiConfig[key], Number(e.currentTarget.value));
            saveSasayakiConfig();
          }}
        />
        <input
          type="color"
          class="size-5 cursor-pointer appearance-none rounded-full bg-transparent p-0 ring-1 ring-base-content/30 ring-offset-2 ring-offset-base-100 [&::-webkit-color-swatch-wrapper]:p-0 [&::-webkit-color-swatch]:rounded-full [&::-webkit-color-swatch]:border-0"
          value={colorHex(sasayakiConfig[key])}
          oninput={(e) => {
            sasayakiConfig[key] = e.currentTarget.value + sasayakiConfig[key].slice(7);
            saveSasayakiConfig();
          }}
        />
      </div>
    </SettingRow>
  {/each}
{/snippet}

{#if compact}
  <SettingGroup>
    {@render settings()}
    {@render controlBar()}
  </SettingGroup>
  <SettingGroup>
    <span class="text-xs font-medium text-base-content/60">Paragraph Mode</span>
    {@render paragraphMode()}
  </SettingGroup>
  <SettingGroup>
    <button class="flex items-center justify-between gap-2 text-left text-sm" onclick={() => (showColors = !showColors)}>
      Highlight
      <span class="flex items-center gap-2 text-xs text-base-content/60">
        {#each themes as [title, , background] (title)}
          {title.split(" ")[0]}
          <span class="size-4 rounded-full ring-1 ring-base-content/30" style:background-color={sasayakiConfig[background]}></span>
        {/each}
        {#if showColors}
          <ChevronDown class="size-4" />
        {:else}
          <ChevronRight class="size-4" />
        {/if}
      </span>
    </button>
    {#if showColors}
      {#each themes as [title, text, background] (title)}
        <span class="text-xs font-medium text-base-content/60">{title}</span>
        {@render colors(text, background)}
      {/each}
    {/if}
  </SettingGroup>
{:else}
  <SettingsSection title="Settings">{@render settings()}</SettingsSection>
  <SettingsSection title="Paragraph Mode">{@render paragraphMode()}</SettingsSection>
  <SettingsSection title="Control Bar">{@render controlBar()}</SettingsSection>
  {#each themes as [title, text, background] (title)}
    <SettingsSection {title}>{@render colors(text, background)}</SettingsSection>
  {/each}
{/if}
