<script lang="ts">
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import type { BookInfo, SasayakiMatchData, SasayakiPlayback } from "./types";

  type Progress =
    | { state: "downloading"; fraction: number }
    | { state: "transcribing"; through: number; duration: number; remaining: number | null }
    | { state: "aligning" };
  type Transcript = { through: number; duration: number };

  let { onMatched }: { onMatched: () => void } = $props();

  let dialog = $state<HTMLDialogElement>();
  let bookId = $state("");
  let canTranscribe = $state<boolean>();
  let useAudio = $state(false);
  let subtitleFile = $state<string | null>(null);
  let audioFile = $state<string | null>(null);
  let isMatching = $state(false);
  let progress = $state<Progress | null>(null);
  let characterCount = $state(0);
  let matchData = $state<SasayakiMatchData | null>(null);
  let transcript = $state<Transcript | null>(null);
  let matchError = $state("");

  const file = $derived(useAudio ? audioFile : subtitleFile);
  const resumable = $derived(
    useAudio &&
      transcript !== null &&
      transcript.through > 0 &&
      !(transcript.duration > 0 && transcript.through + 1.5 >= transcript.duration),
  );

  export async function show(id: string) {
    bookId = id;
    if (!isMatching) {
      useAudio = false;
      subtitleFile = null;
      matchError = "";
    }
    canTranscribe ??= (await invoke<string>("sasayaki_transcriber_status")) === "available";
    if (canTranscribe) {
      const playback = await invoke<SasayakiPlayback | null>("sasayaki_load_playback", { id });
      audioFile = playback?.audioPath ?? null;
    }
    await reload();
    dialog?.showModal();
  }

  async function reload() {
    [matchData, transcript, characterCount] = await Promise.all([
      invoke<SasayakiMatchData | null>("sasayaki_load_match", { id: bookId }),
      canTranscribe ? invoke<Transcript | null>("sasayaki_load_transcript", { id: bookId }) : null,
      invoke<BookInfo>("load_book_info", { id: bookId }).then((info) => info.characterCount),
    ]);
  }

  async function pickFile() {
    const extensions = useAudio ? ["mp3", "m4b", "m4a", "mp4"] : ["srt", "txt"];
    const result = await open({ filters: [{ name: useAudio ? "Audio" : "Subtitles", extensions }] });
    if (!result) return;
    if (useAudio) audioFile = result;
    else subtitleFile = result;
  }

  async function start() {
    isMatching = true;
    matchError = "";
    const onProgress = new Channel<Progress>((message) => (progress = message));
    try {
      if (useAudio) await invoke("sasayaki_transcribe", { id: bookId, path: file, onProgress });
      else await invoke("sasayaki_match", { id: bookId, path: file });
      onMatched();
    } catch (e) {
      matchError = String(e);
    } finally {
      isMatching = false;
      progress = null;
    }
    await reload();
  }

  function pause() {
    if (isMatching && useAudio) invoke("sasayaki_pause_transcription");
  }

  async function clearTranscript() {
    if (
      !(await ask("Only clears transcription data, match is kept.", {
        title: "Clear transcription?",
        kind: "warning",
        okLabel: "Clear",
        cancelLabel: "Cancel",
      }))
    )
      return;
    await invoke("sasayaki_clear_transcript", { id: bookId });
    transcript = null;
    matchError = "";
  }

  function progressLabel(progress: Progress | null): string {
    switch (progress?.state) {
      case "downloading":
        return `Downloading model… ${Math.floor(progress.fraction * 100)}%`;
      case "transcribing":
        return `Transcribing ${timeString(progress.through)} / ${timeString(progress.duration)}`;
      case "aligning":
        return "Aligning…";
      default:
        return "Matching…";
    }
  }

  function timeString(seconds: number): string {
    const total = Math.round(seconds);
    const minutes = String(Math.floor((total % 3600) / 60)).padStart(2, "0");
    return `${Math.floor(total / 3600)}:${minutes}:${String(total % 60).padStart(2, "0")}`;
  }

  function coverage(data: SasayakiMatchData): string {
    const matched = data.matches.reduce((sum, match) => sum + match.length, 0);
    return `${matched}/${characterCount} (${((matched / characterCount) * 100).toFixed(1)}%)`;
  }

  function fileName(path: string): string {
    return path.split(/[\\/]/).pop() ?? path;
  }
</script>

<dialog class="modal" bind:this={dialog} onclose={pause}>
  <div class="modal-box">
    <h3 class="mb-4 text-base font-semibold">Match</h3>
    <div class="flex flex-col gap-4">
      <div class="join w-full">
        <button
          class="btn join-item btn-sm flex-1 {useAudio ? '' : 'btn-active'}"
          disabled={isMatching}
          onclick={() => (useAudio = false)}
        >
          Subtitles
        </button>
        <button
          class="btn join-item btn-sm flex-1 {useAudio ? 'btn-active' : ''}"
          disabled={!canTranscribe || isMatching}
          onclick={() => (useAudio = true)}
        >
          Transcription
        </button>
      </div>

      <div class="flex items-center justify-between gap-3">
        <span class="min-w-0 truncate text-sm">{file ? fileName(file) : "No file selected"}</span>
        <button class="btn btn-outline btn-sm" onclick={pickFile} disabled={isMatching}>Open</button>
      </div>

      {#if isMatching}
        <div class="flex items-center gap-3">
          <span class="loading loading-spinner loading-sm"></span>
          <div class="flex flex-col">
            <span class="text-sm tabular-nums">{progressLabel(progress)}</span>
            {#if progress?.state === "transcribing" && progress.remaining !== null}
              <span class="text-xs text-base-content/60">
                about {Math.ceil(progress.remaining / 60)} min left
              </span>
            {/if}
          </div>
        </div>
        {#if useAudio}
          <button class="btn btn-sm" onclick={pause}>Pause</button>
        {/if}
      {:else}
        <button class="btn btn-neutral btn-sm" onclick={start} disabled={!file}>
          {resumable ? "Resume" : "Match"}
        </button>
      {/if}

      {#if matchError}
        <span class="select-text text-sm text-error">{matchError}</span>
      {/if}

      {#if matchData || transcript}
        <div class="flex flex-col gap-2 border-t border-base-300 pt-3">
          <span class="text-xs text-base-content/60">Current Match</span>
          {#if matchData && characterCount > 0}
            <div class="flex items-center justify-between gap-2 text-sm">
              <span>Coverage</span>
              <span class="font-semibold tabular-nums">{coverage(matchData)}</span>
            </div>
          {/if}
          {#if transcript && transcript.duration > 0}
            <div class="flex items-center justify-between gap-2 text-sm">
              <span>Transcribed</span>
              <span class="font-semibold tabular-nums">
                {timeString(transcript.through)} / {timeString(transcript.duration)}
              </span>
            </div>
          {/if}
          {#if transcript}
            <button class="btn btn-ghost btn-sm text-error" onclick={clearTranscript} disabled={isMatching}>
              Clear Transcription
            </button>
          {/if}
        </div>
      {/if}
    </div>
    <div class="modal-action">
      <form method="dialog">
        <button class="btn btn-sm">Done</button>
      </form>
    </div>
  </div>
  <form method="dialog" class="modal-backdrop">
    <button>close</button>
  </form>
</dialog>
