<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import {
    configureSharing,
    initializeSharing,
    sharingConfig,
    type RemoteDictionary,
    type RemoteStatus,
    type SharingSettings,
  } from "./sharingConfig.svelte";

  let { onImport }: { onImport: () => Promise<void> } = $props();

  let draft = $state<SharingSettings>({ ...sharingConfig });
  let loaded = $state(false);
  let loading = $state(false);
  let applying = $state(false);
  let connecting = $state(false);
  let importingId = $state<string | null>(null);
  let dictionaries = $state<RemoteDictionary[]>([]);
  let remoteStatus = $state<RemoteStatus | null>(null);
  let error = $state("");
  let notice = $state("");

  const dirty = $derived(
    draft.remoteAddress !== sharingConfig.remoteAddress ||
    draft.remoteApiUrl !== sharingConfig.remoteApiUrl ||
    draft.lookupSource !== sharingConfig.lookupSource ||
    draft.serverEnabled !== sharingConfig.serverEnabled ||
    draft.serverPort !== sharingConfig.serverPort,
  );
  const busy = $derived(applying || connecting || importingId !== null);
  const validPort = $derived(Number.isInteger(draft.serverPort) && draft.serverPort > 0 && draft.serverPort <= 65535);
  const serverRelay = $derived(`ws://127.0.0.1:${sharingConfig.serverPort}/link`);
  const serverApi = $derived(`http://127.0.0.1:${sharingConfig.serverPort}`);

  onMount(() => {
    loadSettings();
  });

  async function loadSettings() {
    loading = true;
    error = "";
    try {
      await initializeSharing();
      draft = { ...sharingConfig };
      loaded = true;
    } catch (reason) {
      error = String(reason);
    } finally {
      loading = false;
    }
  }

  async function apply() {
    applying = true;
    error = "";
    notice = "";
    try {
      await configureSharing({
        ...$state.snapshot(draft),
        remoteAddress: draft.remoteAddress.trim(),
        remoteApiUrl: draft.remoteApiUrl.trim(),
      });
      draft = { ...sharingConfig };
      dictionaries = [];
      remoteStatus = null;
      notice = "Sharing settings applied.";
    } catch (reason) {
      error = String(reason);
    } finally {
      applying = false;
    }
  }

  async function refreshRemote() {
    connecting = true;
    error = "";
    notice = "";
    dictionaries = [];
    remoteStatus = null;
    try {
      const [status, available] = await Promise.all([
        invoke<RemoteStatus>("sharing_remote_status"),
        invoke<RemoteDictionary[]>("sharing_remote_dictionaries"),
      ]);
      remoteStatus = status;
      dictionaries = available;
    } catch (reason) {
      error = String(reason);
    } finally {
      connecting = false;
    }
  }

  async function importDictionary(dictionary: RemoteDictionary) {
    importingId = dictionary.id;
    error = "";
    notice = "";
    try {
      const summary = await invoke<{ imported: string[]; failed: string[] }>("sharing_import_dictionary", { id: dictionary.id });
      if (summary.imported.length) {
        await onImport();
        notice = `Imported ${summary.imported.join(", ")}.`;
      }
      if (summary.failed.length) error = summary.failed.join("\n");
    } catch (reason) {
      error = String(reason);
    } finally {
      importingId = null;
    }
  }

  async function copyAddress(address: string) {
    error = "";
    try {
      await navigator.clipboard.writeText(address);
      notice = "Address copied.";
    } catch (reason) {
      error = String(reason);
    }
  }
</script>

<SettingsSection title="Dictionary Sharing">
  <p class="text-sm text-base-content/70">
    Use the hachidori-anki relay to look up words in Hachidori and download its dictionaries,
    or share Hoshi Reader's dictionaries with another app.
  </p>

  {#if !loaded}
    <button class="btn btn-sm self-start" disabled={loading} onclick={loadSettings}>
      {#if loading}<span class="loading loading-xs loading-spinner"></span>Loading Settings{:else}Try Again{/if}
    </button>
  {/if}

  <fieldset class="flex flex-col gap-4" disabled={!loaded || busy}>
    <label class="flex flex-col gap-1.5 text-sm">
      Lookup Source
      <select class="select select-sm w-full" bind:value={draft.lookupSource}>
        <option value="local">Hoshi Reader dictionaries</option>
        <option value="remote">Other app through the relay</option>
      </select>
    </label>

    <label class="flex flex-col gap-1.5 text-sm">
      Lookup Relay Address
      <input class="input input-sm w-full font-mono" type="url" bind:value={draft.remoteAddress} placeholder="ws://127.0.0.1:8771/link" spellcheck="false" />
    </label>

    <label class="flex flex-col gap-1.5 text-sm">
      Dictionary API Address
      <input class="input input-sm w-full font-mono" type="url" bind:value={draft.remoteApiUrl} placeholder="http://127.0.0.1:19633" spellcheck="false" />
    </label>

    <label class="flex items-center justify-between gap-4 text-sm">
      Share Hoshi Reader Dictionaries
      <input class="toggle toggle-primary toggle-sm" type="checkbox" bind:checked={draft.serverEnabled} />
    </label>

    {#if draft.serverEnabled}
      <label class="flex items-center justify-between gap-4 text-sm">
        Sharing Port
        <input class="input input-sm w-28" type="number" min="1" max="65535" step="1" bind:value={draft.serverPort} />
      </label>
      <p class="text-xs text-base-content/70">The server listens on this computer only while Hoshi Reader is open.</p>
    {/if}
  </fieldset>

  <div class="flex flex-wrap items-center gap-3">
    <button
      class="btn btn-primary btn-sm"
      onclick={apply}
      disabled={!loaded || busy || !dirty || !validPort || !draft.remoteAddress.trim() || !draft.remoteApiUrl.trim()}
    >
      {#if applying}<span class="loading loading-xs loading-spinner"></span>{/if}
      Apply
    </button>
    {#if dirty}
      <p class="text-xs text-base-content/70">Apply changes before connecting to the other app.</p>
    {/if}
  </div>

  {#if loaded && sharingConfig.serverEnabled}
    <div class="flex flex-col gap-3 rounded-box border border-base-300 p-3">
      <p class="text-sm font-medium">Hoshi Reader Sharing Addresses</p>
      <div class="flex flex-col gap-1.5 text-xs">
        <label for="hoshi-sharing-relay">Lookup Relay</label>
        <div class="flex min-w-0 gap-2">
          <input id="hoshi-sharing-relay" class="input input-sm min-w-0 flex-1 font-mono" readonly value={serverRelay} />
          <button class="btn btn-sm" onclick={() => copyAddress(serverRelay)} aria-label="Copy Hoshi Reader lookup relay address">Copy</button>
        </div>
      </div>
      <div class="flex flex-col gap-1.5 text-xs">
        <label for="hoshi-sharing-api">Dictionary API</label>
        <div class="flex min-w-0 gap-2">
          <input id="hoshi-sharing-api" class="input input-sm min-w-0 flex-1 font-mono" readonly value={serverApi} />
          <button class="btn btn-sm" onclick={() => copyAddress(serverApi)} aria-label="Copy Hoshi Reader dictionary API address">Copy</button>
        </div>
      </div>
    </div>
  {/if}

  <div class="flex flex-col gap-3 border-t border-base-300 pt-4">
    <button class="btn btn-outline btn-sm self-start" onclick={refreshRemote} disabled={!loaded || busy || dirty}>
      {#if connecting}<span class="loading loading-xs loading-spinner"></span>{/if}
      Connect and List Dictionaries
    </button>
    {#if remoteStatus}
      <p class="text-sm" role="status">
        Connected to {remoteStatus.name} {remoteStatus.version} · {remoteStatus.dictionaryCount} dictionaries
      </p>
      {#if !dictionaries.length}
        <p class="text-sm text-base-content/70">The other app has no dictionaries available to download.</p>
      {/if}
    {/if}
    {#if dictionaries.length}
      <ul class="list rounded-box border border-base-300">
        {#each dictionaries as dictionary (dictionary.id)}
          <li class="list-row items-center">
            <div class="list-col-grow min-w-0">
              <p class="truncate text-sm font-medium">{dictionary.title}</p>
              {#if dictionary.revision}<p class="truncate text-xs text-base-content/70">{dictionary.revision}</p>{/if}
            </div>
            <button class="btn btn-sm" onclick={() => importDictionary(dictionary)} disabled={busy || dirty}>
              {#if importingId === dictionary.id}
                <span class="loading loading-xs loading-spinner"></span>
                Importing
              {:else}
                Download and Import
              {/if}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    {#if importingId !== null}
      <p class="text-xs text-base-content/70" role="status">Downloading and importing the dictionary. Large dictionaries may take a few minutes.</p>
    {/if}
  </div>

  {#if notice}<p class="select-text text-sm" role="status">{notice}</p>{/if}
  {#if error}
    <div class="alert alert-error" role="alert">
      <p class="select-text whitespace-pre-wrap text-sm">{error}</p>
    </div>
  {/if}
</SettingsSection>
