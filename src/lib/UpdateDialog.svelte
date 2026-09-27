<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let dialog = $state<HTMLDialogElement>();
  let version = $state("");
  let installing = $state(false);
  let progress = $state<{ downloaded: number; total: number | null } | null>(null);
  let error = $state("");

  const downloaded = $derived(progress !== null && progress.total !== null && progress.downloaded >= progress.total);

  let lastCheck = 0;

  function check() {
    if (dialog?.open || Date.now() - lastCheck < 24 * 60 * 60 * 1000) return;
    lastCheck = Date.now();
    invoke<string | null>("check_update")
      .then((available) => {
        if (!available) return;
        version = available;
        dialog?.showModal();
      })
      .catch(() => {});
  }

  check();

  $effect(() => {
    const unlisten = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) check();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  $effect(() => {
    const unlisten = listen<{ downloaded: number; total: number | null }>("update://progress", ({ payload }) => {
      progress = payload;
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  function megabytes(bytes: number) {
    return (bytes / 1024 / 1024).toFixed(1);
  }

  async function install() {
    installing = true;
    progress = null;
    error = "";
    try {
      await invoke("install_update");
    } catch (e) {
      error = String(e);
      installing = false;
    }
  }
</script>

<dialog class="modal" bind:this={dialog} oncancel={(e) => installing && e.preventDefault()}>
  <div class="modal-box">
    <h3 class="mb-2 text-base font-semibold">Update Available</h3>
    <p class="text-sm text-base-content/70">Hoshi Reader {version} is available.</p>
    {#if installing}
      <div class="mt-4 flex flex-col gap-1.5">
        {#if progress?.total}
          <progress class="progress progress-primary w-full" value={progress.downloaded} max={progress.total}></progress>
        {:else}
          <progress class="progress progress-primary w-full"></progress>
        {/if}
        <span class="text-xs text-base-content/60 tabular-nums">
          {#if downloaded}
            Installing…
          {:else if progress?.total}
            Downloading… {megabytes(progress.downloaded)} / {megabytes(progress.total)} MB
          {:else}
            Downloading…
          {/if}
        </span>
      </div>
    {/if}
    {#if error}
      <p class="mt-3 select-text text-sm text-error">{error}</p>
    {/if}
    <div class="modal-action">
      <form method="dialog">
        <button class="btn btn-sm" disabled={installing}>Later</button>
      </form>
      <button class="btn btn-primary btn-sm" disabled={installing} onclick={install}>Install & Restart</button>
    </div>
  </div>
</dialog>
