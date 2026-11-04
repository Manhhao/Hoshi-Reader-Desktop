<script lang="ts">
  import { ChevronDown, ChevronRight } from "@lucide/svelte";
  import { colorAlpha, colorHex, sasayakiConfig, saveSasayakiConfig, withAlpha } from "./sasayakiConfig.svelte";
  import SettingGroup from "./SettingGroup.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingRow from "./SettingRow.svelte";
  import SettingToggle from "./SettingToggle.svelte";
  import SettingStepper from "./SettingStepper.svelte";
  import SettingSlider from "./SettingSlider.svelte";

  let { compact = false }: { compact?: boolean } = $props();

  let showColors = $state(false);

  const themes = [
    ["Light Theme", "sasayakiTextColor", "sasayakiBackgroundColor", "sasayakiUnderlineColor"],
    ["Dark Theme", "sasayakiDarkTextColor", "sasayakiDarkBackgroundColor", "sasayakiDarkUnderlineColor"],
  ] as const;

  type Theme = (typeof themes)[number];

  const underline = $derived(sasayakiConfig.sasayakiHighlightStyle === "Underline");

  function colorRows([, text, background, line]: Theme): [string, Theme[1 | 2 | 3]][] {
    return [["Text Color", text], underline ? ["Underline Color", line] : ["Background Color", background]];
  }
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
  <SettingToggle
    label="Track Statistics in Background"
    {compact}
    bind:checked={sasayakiConfig.sasayakiBackgroundStatistics}
    onchange={saveSasayakiConfig}
  />
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

{#snippet highlightStyle()}
  <SettingRow label="Highlight Style" {compact}>
    <div class="join">
      {#each ["Background", "Underline"] as const as style (style)}
        <button
          class="btn join-item btn-sm {sasayakiConfig.sasayakiHighlightStyle === style ? 'btn-active' : ''}"
          onclick={() => {
            sasayakiConfig.sasayakiHighlightStyle = style;
            saveSasayakiConfig();
          }}
        >
          {style}
        </button>
      {/each}
    </div>
  </SettingRow>
  {#if underline}
    <SettingSlider
      label="Thickness"
      {compact}
      bind:value={sasayakiConfig.sasayakiUnderlineThickness}
      display={`${sasayakiConfig.sasayakiUnderlineThickness}px`}
      min={0.5}
      max={2.5}
      step={0.25}
      onchange={saveSasayakiConfig}
    />
  {/if}
{/snippet}

{#snippet colors(theme: Theme)}
  {#each colorRows(theme) as [label, key] (key)}
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
        {#each themes as [title, , background, line] (title)}
          {title.split(" ")[0]}
          <span
            class="size-4 rounded-full ring-1 ring-base-content/30"
            style:background-color={sasayakiConfig[underline ? line : background]}
          ></span>
        {/each}
        {#if showColors}
          <ChevronDown class="size-4" />
        {:else}
          <ChevronRight class="size-4" />
        {/if}
      </span>
    </button>
    {#if showColors}
      {@render highlightStyle()}
      {#each themes as theme (theme[0])}
        <span class="text-xs font-medium text-base-content/60">{theme[0]}</span>
        {@render colors(theme)}
      {/each}
    {/if}
  </SettingGroup>
{:else}
  <SettingsSection title="Settings">{@render settings()}</SettingsSection>
  <SettingsSection title="Paragraph Mode">{@render paragraphMode()}</SettingsSection>
  <SettingsSection title="Control Bar">{@render controlBar()}</SettingsSection>
  <SettingsSection title="Anki">
    <SettingRow label="Audio Format">
      <select class="select select-sm w-28" bind:value={sasayakiConfig.sasayakiAudioFormat} onchange={saveSasayakiConfig}>
        <option value="mp3">MP3</option>
        <option value="opus">Opus</option>
      </select>
    </SettingRow>
  </SettingsSection>
  <SettingsSection title="Highlight">{@render highlightStyle()}</SettingsSection>
  {#each themes as theme (theme[0])}
    <SettingsSection title={theme[0]}>{@render colors(theme)}</SettingsSection>
  {/each}
{/if}
