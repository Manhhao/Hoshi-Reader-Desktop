<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import { persisted } from "./persisted.svelte";

  const seen = persisted("changelog", { lastVersion: "" });
  const releases = "https://api.github.com/repos/Manhhao/Hoshi-Reader-Desktop/releases";

  let dialog = $state<HTMLDialogElement>();
  let version = $state("");
  let title = $state("");
  let notes = $state("");

  function markSeen() {
    seen.config.lastVersion = version;
    seen.save();
  }

  async function load() {
    version = await getVersion();
    if (!seen.config.lastVersion) return markSeen();
    if (seen.config.lastVersion === version) return;
    const response = await fetch(`${releases}/tags/v${version}`, {
      headers: { Accept: "application/vnd.github.html+json" },
    });
    if (response.status === 404) return markSeen();
    if (!response.ok) return;
    const release = await response.json();
    title = release.name || release.tag_name;
    notes = release.body_html ?? "";
    if (!notes) return markSeen();
    dialog?.showModal();
  }

  load().catch(() => {});

  function openLink(e: MouseEvent) {
    const link = (e.target as HTMLElement).closest("a");
    if (!link) return;
    e.preventDefault();
    invoke("open_external", { url: link.href });
  }
</script>

<dialog class="modal" bind:this={dialog} onclose={markSeen}>
  <div class="modal-box max-w-xl">
    <h3 class="mb-3 text-base font-semibold">{title}</h3>
    <div
      class="max-h-[60vh] overflow-y-auto text-sm [&_a]:link [&_code]:rounded [&_code]:bg-base-200 [&_code]:px-1 [&_h1]:mt-3 [&_h1]:font-semibold [&_h2]:mt-3 [&_h2]:font-semibold [&_h3]:mt-3 [&_h3]:font-semibold [&_li]:my-0.5 [&_ol]:list-decimal [&_ol]:pl-5 [&_p]:my-2 [&_ul]:list-disc [&_ul]:pl-5"
      onclick={openLink}
    >
      {@html notes}
    </div>
    <div class="modal-action">
      <form method="dialog">
        <button class="btn btn-primary btn-sm">Continue</button>
      </form>
    </div>
  </div>
</dialog>
