<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import { Plus, Trash2 } from "@lucide/svelte";
  import { defaultFonts, readerConfig, saveReaderConfig } from "./readerConfig.svelte";
  import type { ProgressCount } from "./readerConfig.svelte";
  import SettingGroup from "./SettingGroup.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingRow from "./SettingRow.svelte";
  import SettingToggle from "./SettingToggle.svelte";
  import SettingStepper from "./SettingStepper.svelte";
  import SettingSlider from "./SettingSlider.svelte";
  import type { FontInfo } from "./types";

  let { compact = false, fonts = $bindable([]), onRestyle, onReload, onPopupResize }: {
    compact?: boolean;
    fonts?: FontInfo[];
    onRestyle?: () => void;
    onReload?: () => void;
    onPopupResize?: () => void;
  } = $props();

  const screenWidth = screen.width;
  const screenHeight = screen.height;
  const minWidth = 200 + ((screenWidth - 200) % 50);
  const minHeight = 200 + ((screenHeight - 200) % 50);

  let tab = $state<"Layout" | "Display" | "Popup">("Layout");
  const progressCounts = $derived<ProgressCount[]>(
    readerConfig.continuousMode ? ["Off", "Characters"] : ["Off", "Characters", "Pages"],
  );

  function save(update?: () => void) {
    saveReaderConfig();
    update?.();
  }

  async function refreshFonts() {
    fonts = await invoke("list_fonts");
  }

  refreshFonts();

  function pickFont(name: string) {
    if (readerConfig.selectedFont === name) return;
    readerConfig.selectedFont = name;
    readerConfig.selectedFontFile = fonts.find((font) => font.name === name)?.fileName ?? "";
    save(onReload);
  }

  async function importFont() {
    const path = await open({
      filters: [{ name: "Font", extensions: ["ttf", "otf", "ttc", "woff", "woff2"] }],
    });
    if (!path) return;
    await invoke("import_fonts", { paths: [path] });
    await refreshFonts();
  }

  async function deleteSelectedFont() {
    if (!(await ask(`Delete "${readerConfig.selectedFont}"?`, { kind: "warning" }))) return;
    await invoke("delete_font", { fileName: readerConfig.selectedFontFile });
    readerConfig.selectedFont = defaultFonts[0];
    readerConfig.selectedFontFile = "";
    saveReaderConfig();
    await refreshFonts();
  }

  const customTheme = $derived(readerConfig.customThemes[readerConfig.customTheme]);
  let themeDialog = $state<HTMLDialogElement>();
  let themeName = $state("");

  function openThemeDialog() {
    themeName = `Theme ${readerConfig.customThemes.length + 1}`;
    themeDialog?.showModal();
  }

  function createTheme() {
    readerConfig.customThemes.push({ ...customTheme, name: themeName.trim() });
    readerConfig.customTheme = readerConfig.customThemes.length - 1;
    saveReaderConfig();
  }

  function deleteCustomTheme() {
    readerConfig.customThemes.splice(readerConfig.customTheme, 1);
    readerConfig.customTheme = Math.min(readerConfig.customTheme, readerConfig.customThemes.length - 1);
    saveReaderConfig();
  }
</script>

{#snippet themePicker()}
  <div class="join w-full">
    {#each ["System", "Light", "Dark", "Sepia", "Custom"] as const as theme (theme)}
      <button
        class="btn join-item btn-sm flex-1 px-0 {readerConfig.theme === theme ? 'btn-active' : ''}"
        onclick={() => {
          readerConfig.theme = theme;
          saveReaderConfig();
        }}
      >
        {theme}
      </button>
    {/each}
  </div>
{/snippet}

{#snippet themeOptions()}
  {#if readerConfig.theme === "System"}
    <SettingToggle
      label="Use Sepia as Light Theme"
      {compact}
      bind:checked={readerConfig.systemLightSepia}
      onchange={saveReaderConfig}
    />
  {/if}
  {#if readerConfig.theme === "Sepia"}
    <SettingToggle
      label="Invert in System Dark Theme"
      {compact}
      bind:checked={readerConfig.sepiaInvertInDark}
      onchange={saveReaderConfig}
    />
  {/if}
  {#if readerConfig.theme === "Custom"}
    <SettingRow label="Theme" {compact}>
      <div class="flex items-center gap-2">
        <select
          class="select select-sm w-36"
          value={readerConfig.customTheme}
          onchange={(e) => {
            readerConfig.customTheme = Number(e.currentTarget.value);
            saveReaderConfig();
          }}
        >
          {#each readerConfig.customThemes as theme, index (index)}
            <option value={index}>{theme.name}</option>
          {/each}
        </select>
        <button class="btn btn-sm btn-square" aria-label="Add custom theme" onclick={openThemeDialog}>
          <Plus class="size-4" />
        </button>
        <button
          class="btn btn-ghost btn-sm btn-square text-base-content/60 hover:text-error"
          aria-label="Delete custom theme"
          disabled={readerConfig.customThemes.length === 1}
          onclick={deleteCustomTheme}
        >
          <Trash2 class="size-4" />
        </button>
      </div>
    </SettingRow>
    <SettingRow label="Interface" {compact}>
      <div class="join">
        {#each ["System", "Light", "Dark"] as const as theme (theme)}
          <button
            class="btn join-item btn-sm {customTheme.uiTheme === theme ? 'btn-active' : ''}"
            onclick={() => {
              customTheme.uiTheme = theme;
              saveReaderConfig();
            }}
          >
            {theme}
          </button>
        {/each}
      </div>
    </SettingRow>
    {#each [["Background Color", "backgroundColor"], ["Text Color", "textColor"], ["Info Color", "infoColor"]] as const as [label, key] (key)}
      <SettingRow {label} {compact}>
        <input
          type="color"
          class="size-5 cursor-pointer appearance-none rounded-full bg-transparent p-0 ring-1 ring-base-content/30 ring-offset-2 ring-offset-base-100 [&::-webkit-color-swatch-wrapper]:p-0 [&::-webkit-color-swatch]:rounded-full [&::-webkit-color-swatch]:border-0"
          value={customTheme[key]}
          oninput={(e) => {
            customTheme[key] = e.currentTarget.value;
            saveReaderConfig();
          }}
        />
      </SettingRow>
    {/each}
  {/if}
{/snippet}

{#snippet text()}
  <SettingRow label={compact ? "Orientation" : "Text Orientation"} {compact}>
    <div class="join">
      {#each [["縦", true], ["横", false]] as const as [label, vertical] (label)}
        <button
          class="btn join-item btn-sm {readerConfig.verticalWriting === vertical ? 'btn-active' : ''}"
          onclick={() => {
            if (readerConfig.verticalWriting === vertical) return;
            readerConfig.verticalWriting = vertical;
            save(onReload);
          }}
        >
          {label}
        </button>
      {/each}
    </div>
  </SettingRow>
  <SettingRow label="Font" {compact}>
    <div class="flex min-w-0 flex-1 items-center justify-end gap-2">
      {#if !compact && readerConfig.selectedFontFile}
        <button class="btn btn-ghost btn-sm btn-square text-base-content/60 hover:text-error" onclick={deleteSelectedFont}>
          <Trash2 class="size-4" />
        </button>
      {/if}
      <div class="relative min-w-0 {compact ? 'w-48' : 'w-56'}">
        <select
          class="select select-sm w-full text-transparent [&>option]:text-base-content"
          value={readerConfig.selectedFont}
          onchange={(e) => pickFont(e.currentTarget.value)}
        >
          {#each defaultFonts as font (font)}
            <option value={font}>{font}</option>
          {/each}
          {#each fonts as font (font.fileName)}
            <option value={font.name}>{font.name}</option>
          {/each}
        </select>
        <span class="pointer-events-none absolute inset-y-0 right-3 left-3 truncate text-xs leading-8">{readerConfig.selectedFont}</span>
      </div>
      <button class="btn btn-sm btn-square shrink-0" onclick={importFont}>
        <Plus class="size-4" />
      </button>
    </div>
  </SettingRow>
  <SettingStepper
    label="Font Size"
    {compact}
    bind:value={readerConfig.fontSize}
    min={16}
    max={40}
    onchange={() => save(onRestyle)}
  />
  <SettingRow label="Hide Furigana" {compact}>
    <select
      class="select select-sm w-36"
      bind:value={readerConfig.furiganaMode}
      onchange={() => save(onReload)}
    >
      {#each ["Off", "Dimmed", "Toggle", "Hidden"] as const as mode (mode)}
        <option value={mode}>{mode}</option>
      {/each}
    </select>
  </SettingRow>
{/snippet}

{#snippet paragraph()}
  <SettingToggle
    label="Paragraph Mode"
    {compact}
    bind:checked={readerConfig.paragraphMode}
    onchange={() => save(onReload)}
  />
  {#if readerConfig.paragraphMode}
    <SettingSlider
      label="Max Sentences per Page"
      {compact}
      bind:value={readerConfig.maxSentencesPerPage}
      display={readerConfig.maxSentencesPerPage === 0 ? "Off" : `${readerConfig.maxSentencesPerPage}`}
      min={0}
      max={5}
      step={1}
      onchange={() => save(onReload)}
    />
    {#if readerConfig.maxSentencesPerPage > 0}
      <SettingToggle
        label="Split up Dialogue"
        {compact}
        bind:checked={readerConfig.splitDialogue}
        onchange={() => save(onReload)}
      />
    {/if}
    <SettingToggle
      label="Animation"
      {compact}
      bind:checked={readerConfig.textAnimation}
      onchange={saveReaderConfig}
    />
    {#if readerConfig.textAnimation}
      <SettingSlider
        label="Text Speed"
        {compact}
        bind:value={readerConfig.textSpeed}
        display={`${readerConfig.textSpeed}/s`}
        min={25}
        max={100}
        step={5}
        onchange={saveReaderConfig}
      />
    {/if}
    <SettingToggle
      label="Click to Advance"
      {compact}
      bind:checked={readerConfig.clickToAdvance}
      onchange={saveReaderConfig}
    />
    <SettingToggle
      label="Hide Bookmark"
      {compact}
      bind:checked={readerConfig.paragraphHideBookmark}
      onchange={saveReaderConfig}
    />
  {/if}
{/snippet}

{#snippet spread()}
  <SettingToggle
    label="Two-Page Spread"
    {compact}
    bind:checked={readerConfig.spreadLayout}
    onchange={saveReaderConfig}
  />
  {#if readerConfig.spreadLayout}
    <SettingToggle
      label="Show Progress at Top"
      {compact}
      bind:checked={readerConfig.spreadTopProgress}
      onchange={saveReaderConfig}
    />
    {#if readerConfig.spreadTopProgress}
      <SettingToggle
        label="Show Chapter Title"
        {compact}
        bind:checked={readerConfig.spreadChapterTitle}
        onchange={saveReaderConfig}
      />
    {/if}
  {/if}
{/snippet}

{#snippet continuous()}
  <SettingRow label="Mode" {compact}>
    <div class="join">
      {#each [["Paginated", false], ["Continuous", true]] as const as [label, value] (label)}
        <button
          class="btn join-item btn-sm {readerConfig.continuousMode === value ? 'btn-active' : ''}"
          onclick={() => {
            if (readerConfig.continuousMode === value) return;
            readerConfig.continuousMode = value;
            if (value) readerConfig.paragraphMode = false;
            save(onReload);
          }}
        >
          {label}
        </button>
      {/each}
    </div>
  </SettingRow>
{/snippet}

{#snippet layoutSize()}
  <SettingStepper
    label="Horizontal Padding"
    {compact}
    bind:value={readerConfig.horizontalPadding}
    suffix="%"
    min={0}
    max={50}
    onchange={() => save(onRestyle)}
  />
  <SettingStepper
    label="Vertical Padding"
    {compact}
    bind:value={readerConfig.verticalPadding}
    suffix="%"
    min={0}
    max={50}
    onchange={() => save(onRestyle)}
  />
  <SettingSlider
    label="Max Width"
    {compact}
    value={readerConfig.maxWidth || screenWidth}
    display={`${readerConfig.maxWidth || screenWidth}px`}
    min={minWidth}
    max={screenWidth}
    step={50}
    onchange={(value) => {
      readerConfig.maxWidth = value >= screenWidth ? 0 : value;
      save(onRestyle);
    }}
  />
  <SettingSlider
    label="Max Height"
    {compact}
    value={readerConfig.maxHeight || screenHeight}
    display={`${readerConfig.maxHeight || screenHeight}px`}
    min={minHeight}
    max={screenHeight}
    step={50}
    onchange={(value) => {
      readerConfig.maxHeight = value >= screenHeight ? 0 : value;
      save(onRestyle);
    }}
  />
{/snippet}

{#snippet layoutToggles()}
  {#if !readerConfig.continuousMode}
    <SettingToggle
      label="Avoid Page Break"
      {compact}
      bind:checked={readerConfig.avoidPageBreak}
      onchange={() => save(onRestyle)}
    />
  {/if}
  <SettingToggle
    label="Justify Text"
    {compact}
    bind:checked={readerConfig.justifyText}
    onchange={() => save(onRestyle)}
  />
  <SettingToggle
    label="Blur Images"
    {compact}
    bind:checked={readerConfig.blurImages}
    onchange={() => save(onReload)}
  />
  <SettingToggle
    label="Advanced"
    {compact}
    bind:checked={readerConfig.layoutAdvanced}
    onchange={() => save(onRestyle)}
  />
{/snippet}

{#snippet layoutAdvanced()}
  <SettingSlider
    label="Line Height"
    {compact}
    bind:value={readerConfig.lineHeight}
    display={readerConfig.lineHeight.toFixed(2)}
    min={1}
    max={2.5}
    step={0.05}
    onchange={() => save(onRestyle)}
  />
  <SettingSlider
    label="Character Spacing"
    {compact}
    bind:value={readerConfig.characterSpacing}
    display={`${readerConfig.characterSpacing}%`}
    min={-10}
    max={10}
    step={1}
    onchange={() => save(onRestyle)}
  />
  <SettingSlider
    label="Paragraph Spacing"
    {compact}
    bind:value={readerConfig.paragraphSpacing}
    display={`${readerConfig.paragraphSpacing.toFixed(1)}em`}
    min={0}
    max={3}
    step={0.1}
    onchange={() => save(onRestyle)}
  />
{/snippet}

{#snippet progress()}
  <SettingToggle
    label="Show Progress"
    {compact}
    bind:checked={readerConfig.showProgress}
    onchange={saveReaderConfig}
  />
  <SettingToggle
    label="Show Chapter Progress"
    {compact}
    bind:checked={readerConfig.showChapterProgress}
    onchange={saveReaderConfig}
  />
  {#if readerConfig.showProgress || readerConfig.showChapterProgress || (readerConfig.spreadLayout && readerConfig.spreadTopProgress)}
    <SettingRow label="Count" {compact}>
      <div class="join">
        {#each progressCounts as count (count)}
          <button
            class="btn join-item btn-sm {readerConfig.progressCount === count ? 'btn-active' : ''}"
            onclick={() => {
              readerConfig.progressCount = count;
              saveReaderConfig();
            }}
          >
            {count}
          </button>
        {/each}
      </div>
    </SettingRow>
  {/if}
  {#if readerConfig.showProgress || readerConfig.showChapterProgress}
    <SettingToggle
      label="Show Percentage"
      {compact}
      bind:checked={readerConfig.showPercentage}
      onchange={saveReaderConfig}
    />
  {/if}
{/snippet}

{#snippet display()}
  <SettingToggle
    label="Show Statistics Toggle"
    {compact}
    bind:checked={readerConfig.showStatisticsToggle}
    onchange={saveReaderConfig}
  />
  <SettingToggle
    label="Show Reading Speed"
    {compact}
    bind:checked={readerConfig.showReadingSpeed}
    onchange={saveReaderConfig}
  />
  <SettingToggle
    label="Show Reading Time"
    {compact}
    bind:checked={readerConfig.showReadingTime}
    onchange={saveReaderConfig}
  />
{/snippet}

{#snippet popupSliders()}
  <SettingSlider
    label="Width"
    {compact}
    bind:value={readerConfig.popupWidth}
    min={100}
    max={700}
    step={10}
    onchange={() => save(onPopupResize)}
  />
  <SettingSlider
    label="Height"
    {compact}
    bind:value={readerConfig.popupHeight}
    min={100}
    max={800}
    step={10}
    onchange={() => save(onPopupResize)}
  />
  <SettingSlider
    label="Scale"
    {compact}
    bind:value={readerConfig.popupScale}
    display={readerConfig.popupScale.toFixed(2)}
    min={0.8}
    max={1.5}
    step={0.05}
    onchange={saveReaderConfig}
  />
{/snippet}

{#snippet popupToggles()}
  <SettingToggle
    label="Show Action Bar"
    {compact}
    bind:checked={readerConfig.popupActionBar}
    onchange={saveReaderConfig}
  />
  <SettingToggle
    label="Disable Transparency"
    {compact}
    bind:checked={readerConfig.popupDisableTransparency}
    onchange={saveReaderConfig}
  />
{/snippet}

{#if compact}
  {@render themePicker()}
  {#if readerConfig.theme !== "Light" && readerConfig.theme !== "Dark"}
    <SettingGroup>{@render themeOptions()}</SettingGroup>
  {/if}
  <SettingGroup>{@render text()}</SettingGroup>
  <div class="join mt-1 w-full">
    {#each ["Layout", "Display", "Popup"] as const as name (name)}
      <button class="btn join-item btn-sm flex-1 {tab === name ? 'btn-active' : ''}" onclick={() => (tab = name)}>
        {name}
      </button>
    {/each}
  </div>
  {#if tab === "Layout"}
    <SettingGroup>
      {@render continuous()}
      {#if !readerConfig.continuousMode}
        {@render paragraph()}
      {/if}
    </SettingGroup>
    {#if !readerConfig.continuousMode}
      <SettingGroup>{@render spread()}</SettingGroup>
    {/if}
    <SettingGroup>{@render layoutSize()}</SettingGroup>
    <SettingGroup>{@render layoutToggles()}</SettingGroup>
    {#if readerConfig.layoutAdvanced}
      <SettingGroup>{@render layoutAdvanced()}</SettingGroup>
    {/if}
  {:else if tab === "Display"}
    <SettingGroup>{@render progress()}</SettingGroup>
    <SettingGroup>{@render display()}</SettingGroup>
  {:else}
    <SettingGroup>{@render popupSliders()}</SettingGroup>
    <SettingGroup>{@render popupToggles()}</SettingGroup>
  {/if}
{:else}
  <SettingsSection title="Theme">
    {@render themePicker()}
    {@render themeOptions()}
  </SettingsSection>
  <SettingsSection title="Text">{@render text()}</SettingsSection>
  <SettingsSection title="Layout">
    {@render continuous()}
    {#if !readerConfig.continuousMode}
      {@render paragraph()}
    {/if}
    {@render layoutSize()}
    {@render layoutToggles()}
    {#if readerConfig.layoutAdvanced}
      {@render layoutAdvanced()}
    {/if}
  </SettingsSection>
  {#if !readerConfig.continuousMode}
    <SettingsSection title="Two-Page Spread">{@render spread()}</SettingsSection>
  {/if}
  <SettingsSection title="Progress">{@render progress()}</SettingsSection>
  <SettingsSection title="Display">{@render display()}</SettingsSection>
  <SettingsSection title="Popup">{@render popupSliders()}{@render popupToggles()}</SettingsSection>
{/if}

<dialog class="modal" bind:this={themeDialog}>
  <form method="dialog" class="modal-box" onsubmit={createTheme}>
    <h3 class="mb-4 text-base font-semibold">New Theme</h3>
    <input
      class="input w-full"
      placeholder="Theme name"
      bind:value={themeName}
      required
    />
    <div class="modal-action">
      <button type="button" class="btn btn-sm" onclick={() => themeDialog?.close()}>Cancel</button>
      <button class="btn btn-neutral btn-sm">Create</button>
    </div>
  </form>
  <form method="dialog" class="modal-backdrop">
    <button>close</button>
  </form>
</dialog>
