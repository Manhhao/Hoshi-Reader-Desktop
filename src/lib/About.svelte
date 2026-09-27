<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import { ExternalLink } from "@lucide/svelte";
  import { aboutLicenses } from "./aboutLicenses";
  import artwork from "../../src-tauri/icons/Hoshi.icon/Assets/Artwork.png";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingRow from "./SettingRow.svelte";

  let version = $state("");
  getVersion().then((value) => (version = value));

  let update = $state<"idle" | "checking" | "current" | "installing" | "error">("idle");
  let updateVersion = $state<string | null>(null);

  function openLink(url: string) {
    invoke("open_external", { url });
  }

  async function checkUpdate() {
    update = "checking";
    try {
      updateVersion = await invoke<string | null>("check_update");
      update = updateVersion ? "idle" : "current";
    } catch {
      update = "error";
    }
  }

  async function installUpdate() {
    update = "installing";
    try {
      await invoke("install_update");
    } catch {
      update = "error";
    }
  }
</script>

<div class="card overflow-hidden border border-base-300 bg-base-100">
  <img src={artwork} alt="" draggable="false" class="h-44 w-full object-cover object-[center_66%] [mask-image:linear-gradient(to_bottom,black_30%,transparent)]" />
  <div class="card-body -mt-6">
    <div class="mb-2">
      <h2 class="text-2xl font-semibold">Hoshi Reader Desktop</h2>
      <p class="select-text text-sm text-base-content/60">Version {version}</p>
    </div>
    <SettingRow label="Updates">
      {#if updateVersion}
        <button class="btn btn-primary btn-sm" disabled={update === "installing"} onclick={installUpdate}>{update === "installing" ? "Installing…" : `Install ${updateVersion}`}</button>
      {:else}
        <div class="flex items-center gap-3">
          {#if update === "current"}<span class="text-sm text-base-content/60">Up to date</span>{/if}
          {#if update === "error"}<span class="text-sm text-error">Update failed</span>{/if}
          <button class="btn btn-sm" disabled={update === "checking"} onclick={checkUpdate}>{update === "checking" ? "Checking…" : "Check for Updates"}</button>
        </div>
      {/if}
    </SettingRow>
    <button class="flex items-center justify-between gap-4 text-left text-sm" onclick={() => openLink("https://github.com/Manhhao/Hoshi-Reader-Desktop")}>
      <span>GitHub</span><ExternalLink class="size-4 text-base-content/60" />
    </button>
  </div>
</div>

<details class="group card border border-base-300 bg-base-100">
  <summary class="flex list-none cursor-pointer items-center justify-between gap-4 p-6 text-base font-semibold [&::-webkit-details-marker]:hidden">
    <span>Dependencies and Attribution</span>
    <span class="text-xs font-normal text-base-content/60 group-open:hidden">Show</span>
    <span class="hidden text-xs font-normal text-base-content/60 group-open:inline">Hide</span>
  </summary>
  <div class="flex flex-col gap-6 px-6 pb-6">
    {#each aboutLicenses as section (section.title)}
      <SettingsSection title={section.title} compact>
        {#each section.entries as entry (entry.name)}
          {#if entry.text}
            <details class="py-1">
              <summary class="list-none cursor-pointer text-sm [&::-webkit-details-marker]:hidden">
                {entry.name}<span class="float-right text-xs text-base-content/60">{entry.license}</span>
              </summary>
              <div class="flex flex-col gap-2 pt-3">
                <button class="link self-start text-xs" onclick={() => openLink(entry.url)}>GitHub</button>
                <p class="whitespace-pre-wrap text-xs text-base-content/60">{entry.text}</p>
              </div>
            </details>
          {:else}
            <button class="flex items-center justify-between gap-3 py-1 text-left text-sm" onclick={() => openLink(entry.url)}>
              <span>{entry.name}</span><span class="shrink-0 text-xs text-base-content/60">{entry.license}</span>
            </button>
          {/if}
        {/each}
      </SettingsSection>
    {/each}
  </div>
</details>
