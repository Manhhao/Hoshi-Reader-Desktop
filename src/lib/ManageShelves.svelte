<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import { Menu, Pencil, Plus, Trash2 } from "@lucide/svelte";
  import SettingToggle from "./SettingToggle.svelte";
  import { shellConfig, saveShellConfig } from "./shellConfig.svelte";
  import { createShelf, deleteShelf, moveShelves, renameShelf, shelves } from "./shelves.svelte";
  import { Sortable } from "./sortable.svelte";

  let { onRename }: { onRename: (name: string, newName: string) => void } = $props();

  let dialog = $state<HTMLDialogElement>();
  let newShelfName = $state("");
  let renameDialog = $state<HTMLDialogElement>();
  let renameName = $state("");
  let renameText = $state("");
  const renamedName = $derived(renameText.trim().normalize("NFC"));
  const canRename = $derived(
    renamedName !== "" && !shelves.list.some((shelf) => shelf.name === renamedName),
  );

  const sort = new Sortable({ list: () => shelves.list, commit: moveShelves });

  async function removeShelf(name: string) {
    if (!(await ask(`Delete "${name}"?`, { kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" }))) return;
    await deleteShelf(name);
  }

  function openRename(name: string) {
    renameName = name;
    renameText = name;
    renameDialog?.showModal();
  }

  async function saveRename() {
    if (!canRename) return;
    const newName = renamedName;
    renameDialog?.close();
    await renameShelf(renameName, newName);
    onRename(renameName, newName);
  }

  export function show() {
    dialog?.showModal();
  }

  function add() {
    const name = newShelfName.trim().normalize("NFC");
    if (!name) return;
    createShelf(name);
    newShelfName = "";
  }
</script>

<dialog class="modal" bind:this={dialog}>
  <div class="modal-box flex flex-col gap-5">
    <h3 class="text-base font-semibold">Manage Shelves</h3>

    <SettingToggle label="Author Shelves" bind:checked={shellConfig.showAuthors} onchange={saveShellConfig} />

    <div class="flex flex-col gap-2">
      <h4 class="text-xs font-medium uppercase tracking-wide text-base-content/60">Shelves</h4>
      {#if shelves.list.length === 0}
        <div
          class="flex h-12 items-center justify-center rounded-box border border-base-300 text-sm text-base-content/60"
        >
          No Shelves
        </div>
      {:else}
        <ul class="list rounded-box border border-base-300" use:sort.container>
          {#each shelves.list as shelf, index (shelf.name)}
            <li class="list-row items-center {sort.index === index ? 'opacity-50' : ''}">
              <p class="list-col-grow truncate text-sm">{shelf.name}</p>
              <span class="text-xs tabular-nums text-base-content/50">{shelf.bookIds.length}</span>
              <button
                class="btn btn-ghost btn-xs btn-square text-base-content/50"
                onclick={() => openRename(shelf.name)}
              >
                <Pencil class="size-4" />
              </button>
              <button
                class="btn btn-ghost btn-xs btn-square text-base-content/50"
                onclick={() => removeShelf(shelf.name)}
              >
                <Trash2 class="size-4" />
              </button>
              <span
                role="button"
                tabindex="-1"
                class="cursor-grab touch-none text-base-content/40"
                onpointerdown={(e) => sort.start(index, e)}
              >
                <Menu class="size-4" />
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="flex flex-col gap-2">
      <h4 class="text-xs font-medium uppercase tracking-wide text-base-content/60">Add Shelf</h4>
      <div class="join">
        <input
          class="input input-sm join-item w-full"
          placeholder="Shelf name"
          bind:value={newShelfName}
          onkeydown={(e) => {
            if (e.key === "Enter") add();
          }}
        />
        <button
          class="btn btn-neutral btn-sm join-item"
          onclick={add}
          disabled={newShelfName.trim() === ""}
        >
          <Plus class="size-4" />
        </button>
      </div>
    </div>

    <div class="modal-action mt-0">
      <form method="dialog">
        <button class="btn btn-sm">Done</button>
      </form>
    </div>
  </div>
  <form method="dialog" class="modal-backdrop">
    <button>close</button>
  </form>
</dialog>

<dialog class="modal" bind:this={renameDialog}>
  <div class="modal-box">
    <h3 class="mb-4 text-base font-semibold">Rename Shelf</h3>
    <input
      class="input w-full"
      placeholder="Shelf name"
      bind:value={renameText}
      onkeydown={(e) => {
        if (e.key === "Enter") saveRename();
      }}
    />
    <div class="modal-action">
      <form method="dialog">
        <button class="btn btn-sm">Cancel</button>
      </form>
      <button class="btn btn-neutral btn-sm" disabled={!canRename} onclick={saveRename}>Save</button>
    </div>
  </div>
  <form method="dialog" class="modal-backdrop">
    <button>close</button>
  </form>
</dialog>
