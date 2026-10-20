<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { attachPrompt } from "./syncConfig.svelte";
  import type { GoogleDriveSyncStatus } from "./types";

  let dialog = $state<HTMLDialogElement>();
  let pending = $state(false);
  let attaching = $state(false);
  let error = $state("");

  function update(status: GoogleDriveSyncStatus) {
    if (status.attachPending && !pending) attachPrompt.dismissed = false;
    pending = status.attachPending;
  }

  $effect(() => {
    invoke<GoogleDriveSyncStatus>("gdrive_sync_state").then(update);
    const unlisten = listen<GoogleDriveSyncStatus>("sync://status", ({ payload }) => update(payload));
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  $effect(() => {
    if (!dialog) return;
    if (pending && !attachPrompt.dismissed) {
      if (!dialog.open) dialog.showModal();
    } else if (dialog.open && !attaching) {
      dialog.close();
    }
  });

  async function attach(choice: "merge" | "device" | "cloud") {
    if (choice === "device") {
      const confirmed = await ask(
        "This will permanently delete the library on Google Drive and replace it with the library from this device. Anything not on this device will be lost.",
        { title: "Upload to Google Drive?", kind: "warning", okLabel: "Replace", cancelLabel: "Cancel" },
      );
      if (!confirmed) return;
    } else if (choice === "cloud") {
      const confirmed = await ask(
        "This will permanently delete the library on this device and replace it with the library from Google Drive. Anything not on Google Drive will be lost.",
        { title: "Download from Google Drive?", kind: "warning", okLabel: "Replace", cancelLabel: "Cancel" },
      );
      if (!confirmed) return;
    }
    attaching = true;
    error = "";
    try {
      await invoke("gdrive_attach", { choice });
      dialog?.close();
    } catch (e) {
      error = String(e);
    } finally {
      attaching = false;
    }
  }
</script>

<dialog class="modal" bind:this={dialog} oncancel={() => (attachPrompt.dismissed = true)}>
  <div class="modal-box max-w-sm">
    <h3 class="mb-2 text-base font-semibold">Merge Libraries?</h3>
    <p class="text-sm text-base-content/70">Google Drive already has a library. Merging combines changes from both libraries.</p>
    {#if error}
      <p class="mt-3 select-text whitespace-pre-line text-sm text-error">{error}</p>
    {/if}
    <div class="mt-5 flex flex-col gap-2">
      <button class="btn btn-neutral btn-sm" disabled={attaching} onclick={() => attach("merge")}>Merge</button>
      <button class="btn btn-outline btn-error btn-sm" disabled={attaching} onclick={() => attach("cloud")}>
        Download from Google Drive
      </button>
      <button class="btn btn-outline btn-error btn-sm" disabled={attaching} onclick={() => attach("device")}>
        Upload to Google Drive
      </button>
      <button class="btn btn-sm" disabled={attaching} onclick={() => (attachPrompt.dismissed = true)}>Cancel</button>
    </div>
  </div>
</dialog>
