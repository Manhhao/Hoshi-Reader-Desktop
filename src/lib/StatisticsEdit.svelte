<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { Trash2 } from "@lucide/svelte";
  import { formatNumber, formatUnits } from "./statsModel.svelte";
  import type { ReadingSession, Sessions } from "./types";

  type SessionEntry = { id: string; session: ReadingSession };

  let { onChanged }: { onChanged: () => void } = $props();

  let dialog = $state<HTMLDialogElement>();
  let folder = $state("");
  let bookTitle = $state("");
  let sessionEntries = $state<SessionEntry[]>([]);
  let editing = $state<SessionEntry | null>(null);
  let characters = $state(0);
  let hours = $state(0);
  let minutes = $state(0);

  const dayMonthYearTime = new Intl.DateTimeFormat(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "numeric",
    minute: "numeric",
  });

  const dayMonthTime = new Intl.DateTimeFormat(undefined, {
    day: "numeric",
    month: "short",
    hour: "numeric",
    minute: "numeric",
  });

  export async function show(bookFolder: string, title: string) {
    folder = bookFolder;
    bookTitle = title;
    editing = null;
    await loadSessions();
    dialog?.showModal();
  }

  async function loadSessions() {
    const sessions = await invoke<Sessions>("load_statistics", { folder });
    sessionEntries = Object.entries(sessions)
      .filter(([, change]) => change.value !== null)
      .map(([id, change]) => ({ id, session: change.value! }))
      .sort((first, second) =>
        first.session.startedAt === second.session.startedAt
          ? first.id < second.id ? -1 : 1
          : first.session.startedAt - second.session.startedAt,
      );
  }

  function edit(entry: SessionEntry) {
    editing = entry;
    characters = entry.session.charactersRead;
    const total = Math.round(entry.session.readingTime / 60);
    hours = Math.trunc(total / 60);
    minutes = total % 60;
  }

  async function save() {
    const entry = editing!;
    const totalMinutes = hours * 60 + minutes;
    const readingTime =
      totalMinutes === Math.round(entry.session.readingTime / 60)
        ? entry.session.readingTime
        : totalMinutes * 60;
    const charactersRead = Math.max(characters, 0);
    await invoke("edit_reading_session", {
      folder,
      id: entry.id,
      charactersRead: charactersRead === entry.session.charactersRead ? null : charactersRead,
      readingTime: readingTime === entry.session.readingTime ? null : readingTime,
    });
    editing = null;
    await loadSessions();
    onChanged();
  }

  async function deleteSessions(ids: string[]) {
    await invoke("delete_reading_sessions", { folder, ids });
    await loadSessions();
    onChanged();
  }

  async function deleteSession(entry: SessionEntry) {
    if (!(await ask(`This will delete the session from ${dayMonthYearTime.format(new Date(entry.session.startedAt))}.`, {
      title: "Delete Session?",
      kind: "warning",
      okLabel: "Delete",
      cancelLabel: "Cancel",
    }))) return;
    await deleteSessions([entry.id]);
  }

  async function deleteAll() {
    if (!(await ask("This will delete all recorded statistics for this book.", {
      title: "Delete All Statistics?",
      kind: "warning",
    }))) return;
    await deleteSessions(sessionEntries.map((entry) => entry.id));
    dialog?.close();
  }
</script>

<dialog class="modal" bind:this={dialog}>
  <div class="modal-box flex max-h-[80vh] flex-col gap-4">
    <h3 class="truncate text-base font-semibold">{bookTitle}</h3>

    {#if editing}
      <div class="flex flex-col gap-3">
        <span class="text-xs font-medium tracking-wide text-base-content/60 uppercase">
          {dayMonthTime.format(new Date(editing.session.startedAt))}
        </span>
        <label class="flex items-center justify-between gap-4">
          <span class="text-sm">Characters Read</span>
          <input
            type="number"
            min="0"
            class="input input-sm w-44 tabular-nums"
            bind:value={characters}
          />
        </label>
        <div class="flex items-center justify-between gap-4">
          <span class="text-sm">Reading Time</span>
          <div class="flex w-44 gap-2">
            <label class="input input-sm min-w-0 flex-1">
              <input
                type="number"
                min="0"
                max="23"
                class="min-w-0 flex-1 tabular-nums"
                bind:value={hours}
              />
              <span class="text-base-content/60">h</span>
            </label>
            <label class="input input-sm min-w-0 flex-1">
              <input
                type="number"
                min="0"
                max="59"
                class="min-w-0 flex-1 tabular-nums"
                bind:value={minutes}
              />
              <span class="text-base-content/60">m</span>
            </label>
          </div>
        </div>
        <div class="flex justify-end gap-2">
          <button class="btn btn-ghost btn-sm" onclick={() => (editing = null)}>Cancel</button>
          <button class="btn btn-neutral btn-sm" onclick={save}>Save</button>
        </div>
      </div>
    {:else}
      <div class="flex min-h-0 flex-col gap-2">
        <h4 class="text-xs font-medium tracking-wide text-base-content/60 uppercase">Sessions</h4>
        {#if sessionEntries.length === 0}
          <div
            class="flex h-12 items-center justify-center rounded-box border border-base-300 text-sm text-base-content/60"
          >
            No Statistics
          </div>
        {:else}
          <ul class="list min-h-0 overflow-y-auto rounded-box border border-base-300">
            {#each sessionEntries as entry (entry.id)}
              <li class="list-row items-center has-[button.list-col-grow:hover]:bg-base-200">
                <button class="list-col-grow text-left after:absolute after:inset-0" onclick={() => edit(entry)}>
                  <div class="text-sm">{dayMonthYearTime.format(new Date(entry.session.startedAt))}</div>
                  <div class="text-xs text-base-content/60 tabular-nums">
                    {formatNumber(entry.session.charactersRead)}
                  </div>
                </button>
                <span class="text-sm text-base-content/60 tabular-nums">
                  {formatUnits(entry.session.readingTime)}
                </span>
                <button
                  class="btn btn-ghost btn-xs btn-square relative text-base-content/50"
                  onclick={() => deleteSession(entry)}
                >
                  <Trash2 class="size-4" />
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <div class="modal-action mt-0 justify-between">
        <button class="btn btn-error btn-sm" onclick={deleteAll}>Delete All Statistics</button>
        <form method="dialog">
          <button class="btn btn-sm">Done</button>
        </form>
      </div>
    {/if}
  </div>
  <form method="dialog" class="modal-backdrop">
    <button>close</button>
  </form>
</dialog>
