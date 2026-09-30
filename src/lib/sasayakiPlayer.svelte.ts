import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { isWindows } from "./platform";
import { readerConfig } from "./readerConfig.svelte";
import { schemeUrl } from "./scheme";
import { sasayakiConfig } from "./sasayakiConfig.svelte";
import type { SasayakiImage, SasayakiMatch, SasayakiMatchData, SasayakiPlayback } from "./types";

const SKIP_INTERVAL = 15;

class CueTimeline {
  private cues: SasayakiMatch[];

  constructor(match: SasayakiMatchData | null = null) {
    this.cues = match?.matches ?? [];
  }

  nextCue(time: number): number | null {
    let index = this.findCue(time);
    if (index < this.cues.length && this.cues[index].startTime === time) index += 1;
    return index < this.cues.length ? this.cues[index].startTime : null;
  }

  prevCue(time: number): number | null {
    const index = this.findCue(time);
    return index > 0 ? this.cues[index - 1].startTime : null;
  }

  cueBefore(time: number): SasayakiMatch | null {
    const index = this.findCue(time);
    return index > 0 ? this.cues[index - 1] : null;
  }

  cue(time: number): SasayakiMatch | null {
    const index = this.findCue(time);
    if (index < this.cues.length && Math.abs(this.cues[index].startTime - time) <= 0.01) {
      return this.cues[index];
    }
    if (index === 0) return null;
    const cue = this.cues[index - 1];
    return time <= cue.endTime ? cue : null;
  }

  private findCue(time: number): number {
    let low = 0;
    let high = this.cues.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (this.cues[mid].startTime < time) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    return low;
  }
}

type Bridge = {
  highlightCue: (id: string, reveal: boolean) => void;
  clearCue: () => void;
  scrollToImage: (index: number) => void;
};

type NowPlaying = {
  title: string;
  cover: string | null;
  chapter: (cue: SasayakiMatch) => string | null;
};

export class SasayakiPlayer {
  matchData = $state<SasayakiMatchData | null>(null);
  currentTime = $state(0);
  duration = $state(0);
  isPlaying = $state(false);
  delay = $state(0);
  rate = $state(1);
  volume = $state(1);
  errorMessage = $state<string | null>(null);
  hasAudio = $state(false);
  cover = $state<string | null>(null);

  playback = $state.raw<SasayakiPlayback>({ lastPosition: 0, delay: 0, rate: 1, volume: 1 });
  private timeline = new CueTimeline();
  private unmutedVolume = 1;
  private audio: HTMLAudioElement | null = null;
  private ticker = 0;
  private stopPlaybackTime: number | null = null;
  private pageRange: { start: number; end: number } | null = null;
  private lastUpdate = -1;
  private audioChapters: { start: number; title: string }[] = [];
  private artwork: string | null = null;
  private chapter: string | null = null;
  private unlistenMediaControls: UnlistenFn | null = null;
  private mediaActions: Partial<Record<MediaSessionAction, MediaSessionActionHandler>> = {
    play: () => {
      if (!this.isPlaying) this.togglePlayback();
    },
    pause: () => {
      if (this.isPlaying) this.togglePlayback();
    },
    previoustrack: () => this.prevCue(),
    nexttrack: () => this.nextCue(),
    seekto: (details) => this.scrub(details.seekTime!),
  };

  currentCue: SasayakiMatch | null = null;
  private lastCue: SasayakiMatch | null = null;
  pendingCue: SasayakiMatch | null = null;
  private pendingImage: SasayakiImage | null = null;
  pausedOnImage = false;
  private imageResumeCue: SasayakiMatch | null = null;
  private imagePauseTimer = 0;
  private chapterTransition = true;
  private shouldResume = false;
  private hasPlayedOnce = false;
  private wakeLock: WakeLockSentinel | null = null;

  constructor(
    private id: string,
    private bridge: Bridge,
    private loadChapter: (index: number) => void,
    private getCurrentIndex: () => number,
    private nowPlaying: NowPlaying,
  ) {
    document.addEventListener("visibilitychange", this.onVisibilityChange);
  }

  dispose() {
    document.removeEventListener("visibilitychange", this.onVisibilityChange);
    this.teardown();
  }

  private onVisibilityChange = () => {
    if (document.visibilityState === "visible" && this.isPlaying) this.acquireWakeLock();
  };

  private async acquireWakeLock() {
    if (this.wakeLock || document.visibilityState !== "visible") return;
    try {
      const lock = await navigator.wakeLock?.request("screen");
      if (!lock) return;
      if (!this.isPlaying) {
        lock.release().catch(() => {});
        return;
      }
      lock.addEventListener("release", () => {
        if (this.wakeLock === lock) this.wakeLock = null;
      });
      this.wakeLock = lock;
    } catch {}
  }

  private releaseWakeLock() {
    this.wakeLock?.release().catch(() => {});
    this.wakeLock = null;
  }

  get hasMatch() {
    return this.matchData !== null;
  }

  private get autoScroll() {
    return sasayakiConfig.sasayakiAutoScroll;
  }

  private get pageAdvance() {
    return sasayakiConfig.sasayakiPageAdvance && readerConfig.paragraphMode;
  }



  async load() {
    await this.reloadMatch();
    await this.reloadPlayback();
    this.restoreAudio();
  }

  async reloadMatch() {
    this.matchData = await invoke<SasayakiMatchData | null>("sasayaki_load_match", {
      id: this.id,
    });
    this.timeline = new CueTimeline(this.matchData);
  }

  async reloadPlayback() {
    this.playback = (await invoke<SasayakiPlayback | null>("sasayaki_load_playback", {
      id: this.id,
    })) ?? { lastPosition: 0, delay: 0, rate: 1, volume: 1 };
    this.currentTime = this.playback.lastPosition;
    this.delay = this.playback.delay;
    this.rate = this.playback.rate;
    this.volume = this.playback.volume;
    this.lastUpdate = Math.floor(this.currentTime);
    if (this.audio) {
      this.seek(this.currentTime, false, false);
      this.audio.playbackRate = this.rate;
      this.audio.volume = this.volume;
    }
  }

  setDelay(value: number) {
    this.delay = value;
    this.savePlayback();
    this.updateCue(this.currentTime);
  }

  setRate(value: number) {
    this.rate = value;
    this.savePlayback();
    if (this.audio) this.audio.playbackRate = value;
  }

  setVolume(value: number) {
    this.volume = value;
    this.savePlayback();
    if (this.audio) this.audio.volume = value;
  }

  toggleMute() {
    if (this.volume > 0) {
      this.unmutedVolume = this.volume;
      this.setVolume(0);
    } else {
      this.setVolume(this.unmutedVolume);
    }
  }

  importAudio(path: string) {
    this.teardown();
    this.playback = { ...this.playback, audioPath: path };
    this.savePlayback();
    this.errorMessage = null;
    this.setupPlayer();
  }

  restoreAudio() {
    if (!this.playback.audioPath) return;
    this.setupPlayer();
  }

  cues(chapterIndex: number) {
    return (this.matchData?.matches ?? [])
      .filter((match) => match.chapterIndex === chapterIndex)
      .map((match) => ({ id: match.id, start: match.start, length: match.length }));
  }

  togglePlayback() {
    if (this.pausedOnImage) {
      this.cancelImagePause();
      this.startPlayback();
      return;
    }
    if (this.isPlaying) {
      this.pausePlayback();
    } else {
      this.startPlayback();
      if (this.autoScroll && this.currentCue) this.displayCue(this.currentCue, true);
    }
  }

  nextCue() {
    this.stopPlaybackTime = null;
    const next = this.timeline.nextCue(this.currentCue?.startTime ?? this.currentTime - this.delay);
    if (next === null) return;
    this.seek(next + this.delay);
  }

  prevCue() {
    this.stopPlaybackTime = null;
    const previous =
      this.timeline.prevCue(
        this.currentCue?.startTime ?? Math.max(0, this.currentTime - this.delay),
      ) ?? 0;
    this.seek(previous + this.delay);
  }

  skip(forward: boolean) {
    this.stopPlaybackTime = null;
    if (forward) {
      const target = this.currentTime + SKIP_INTERVAL;
      this.seek(this.duration !== 0 ? Math.min(target, this.duration) : target);
    } else {
      this.seek(Math.max(0, this.currentTime - SKIP_INTERVAL));
    }
  }

  scrub(seconds: number) {
    this.stopPlaybackTime = null;
    this.seek(seconds);
  }

  findCue(chapterIndex: number, offset: number): SasayakiMatch | null {
    const matches = this.matchData?.matches;
    if (!matches) return null;
    let low = 0;
    let high = matches.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      const m = matches[mid];
      if (m.chapterIndex < chapterIndex || (m.chapterIndex === chapterIndex && m.start + m.length <= offset)) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    return low < matches.length &&
      matches[low].chapterIndex === chapterIndex &&
      matches[low].start <= offset
      ? matches[low]
      : null;
  }

  playCue(cue: SasayakiMatch, stop: boolean) {
    this.stopPlaybackTime = null;
    if (this.isPlaying) this.pausePlayback();
    this.seek(cue.startTime + this.delay, true, false, stop ? cue.endTime + this.delay : null);
  }

  prepareTransition() {
    this.shouldResume = this.isPlaying;
    this.chapterTransition = true;
    this.stopPlaybackTime = null;
    this.clearDisplayedCue();
    if (this.isPlaying) this.pausePlayback();
  }

  handleRestoreCompleted(currentIndex: number) {
    if (!this.hasAudio) this.restoreAudio();
    if (!this.hasMatch || !this.chapterTransition) return;

    if (this.pendingImage && this.pendingImage.chapterIndex === currentIndex) {
      const image = this.pendingImage;
      this.chapterTransition = false;
      this.pendingImage = null;
      this.scrollAndPause(image);
      return;
    }

    let cue: SasayakiMatch | null = null;
    let reveal = false;
    const active = this.timeline.cue(this.currentTime - this.delay);
    if (this.pendingCue && this.pendingCue.chapterIndex === currentIndex) {
      cue = this.pendingCue;
      reveal = this.autoScroll && this.hasPlayedOnce;
    } else if (active && active.chapterIndex === currentIndex) {
      cue = active;
      reveal = this.shouldResume && this.autoScroll;
    }

    const resume = this.shouldResume;
    this.chapterTransition = false;
    this.shouldResume = false;
    this.pendingCue = null;

    if (cue) {
      this.displayCue(cue, reveal);
    } else {
      this.clearDisplayedCue();
    }

    if (resume) this.startPlayback();
  }

  handlePageChanged(cueIds: string[], play: boolean | null) {
    const ids = new Set(cueIds);
    const cues = (this.matchData?.matches ?? []).filter((match) => ids.has(match.id));

    this.pageRange = null;
    if (play !== null) {
      this.stopPlaybackTime = null;
      if (this.isPlaying) this.pausePlayback();
    }

    if (!cues.length) return;
    const first = cues.reduce((a, b) => (b.startTime < a.startTime ? b : a));
    const last = cues.reduce((a, b) => (b.startTime < a.startTime ? a : b));
    let end = Math.max(...cues.map((cue) => cue.endTime));
    const next = this.timeline.nextCue(last.startTime);
    if (next !== null) end = Math.min(end, Math.max(next - 0.02, last.startTime));

    let start = first.startTime;
    const prev = this.timeline.cueBefore(first.startTime);
    if (prev && first.start > (prev.chapterIndex === first.chapterIndex ? prev.start + prev.length : 0)) {
      start = Math.min(start, prev.endTime + 0.05);
    }
    this.pageRange = { start, end };

    if (play !== null) this.seek(start + this.delay, play);
  }

  handleImageResult(paused: boolean) {
    if (!this.pausedOnImage) return;
    if (!paused) {
      this.finishImagePause();
      return;
    }
    clearTimeout(this.imagePauseTimer);
    this.imagePauseTimer = window.setTimeout(
      () => this.finishImagePause(),
      sasayakiConfig.sasayakiImagePauseDuration * 1000,
    );
  }

  teardown() {
    this.releaseWakeLock();
    this.cancelImagePause();
    clearInterval(this.ticker);
    this.ticker = 0;
    if (this.audio) {
      this.audio.pause();
      this.audio.removeAttribute("src");
      this.audio.load();
      this.audio = null;
    }
    this.hasAudio = false;
    this.cover = null;
    if (isWindows) {
      this.unlistenMediaControls?.();
      this.unlistenMediaControls = null;
      invoke("media_controls_clear");
    } else {
      navigator.mediaSession.metadata = null;
      for (const action in this.mediaActions) {
        navigator.mediaSession.setActionHandler(action as MediaSessionAction, null);
      }
    }
    this.isPlaying = false;
    this.duration = 0;
    this.stopPlaybackTime = null;
    this.clearDisplayedCue();
  }

  private setupPlayer() {
    const audio = new Audio(schemeUrl("audiobook", this.id) + `?t=${Date.now()}`);
    this.setupMediaSession(audio);
    audio.preload = "auto";
    audio.playbackRate = this.rate;
    audio.volume = this.volume;
    audio.addEventListener("loadedmetadata", () => {
      this.duration = Number.isFinite(audio.duration) ? audio.duration : 0;
      audio.currentTime = this.currentTime;
    });
    audio.addEventListener("ended", () => {
      this.stopPlaybackTime = null;
      this.isPlaying = false;
      this.releaseWakeLock();
      clearInterval(this.ticker);
    });
    audio.addEventListener("error", () => {
      this.errorMessage = "Could not load the audio file.";
    });
    this.audio = audio;
    this.hasAudio = true;
  }

  private setupMediaSession(audio: HTMLAudioElement) {
    this.audioChapters = [];
    this.artwork = this.nowPlaying.cover;
    this.chapter = null;
    this.updateMetadata();
    invoke<{ start: number; title: string }[]>("sasayaki_audio_chapters", { id: this.id }).then(
      (chapters) => {
        if (this.audio !== audio) return;
        this.audioChapters = chapters;
        this.updateChapter();
      },
    );
    const cover = schemeUrl("audiobook", `${this.id}/cover`) + `?t=${Date.now()}`;
    fetch(cover).then((response) => {
      if (!response.ok || this.audio !== audio) return;
      this.cover = cover;
      this.artwork = cover;
      this.updateMetadata();
    });
    if (isWindows) {
      const setPlaying = (playing: boolean) => {
        if (this.audio === audio) invoke("media_controls_playing", { playing });
      };
      audio.addEventListener("play", () => setPlaying(true));
      audio.addEventListener("pause", () => setPlaying(false));
      listen<MediaSessionAction>("hoshi://media-control", (event) =>
        this.mediaActions[event.payload]?.({ action: event.payload }),
      ).then((unlisten) => {
        if (this.audio === audio) this.unlistenMediaControls = unlisten;
        else unlisten();
      });
      return;
    }
    for (const [action, handler] of Object.entries(this.mediaActions)) {
      navigator.mediaSession.setActionHandler(action as MediaSessionAction, handler);
    }
  }

  private updateChapter() {
    const chapter =
      this.audioChapters.findLast((chapter) => chapter.start <= this.currentTime)?.title ??
      (this.currentCue ? this.nowPlaying.chapter(this.currentCue) : null);
    if (chapter === this.chapter) return;
    this.chapter = chapter;
    this.updateMetadata();
  }

  private updateMetadata() {
    if (isWindows) {
      invoke("media_controls_metadata", {
        id: this.id,
        title: this.nowPlaying.title,
        artist: this.chapter ?? "",
      });
      return;
    }
    navigator.mediaSession.metadata = new MediaMetadata({
      title: this.nowPlaying.title,
      artist: this.chapter ?? "",
      artwork: this.artwork ? [{ src: this.artwork }] : [],
    });
  }

  private startPlayback() {
    if (!this.audio) return;
    this.audio.playbackRate = this.rate;
    this.audio.play().catch(() => {});
    this.isPlaying = true;
    this.hasPlayedOnce = true;
    this.acquireWakeLock();
    clearInterval(this.ticker);
    this.ticker = window.setInterval(() => this.tick(this.audio?.currentTime ?? 0), 125);
  }

  private pausePlayback() {
    if (!this.audio) return;
    this.audio.pause();
    this.isPlaying = false;
    this.releaseWakeLock();
    clearInterval(this.ticker);
    this.ticker = 0;
  }

  private tick(seconds: number) {
    const previousTime = this.currentTime;
    this.currentTime = seconds;

    const duration = this.audio?.duration;
    if (duration !== undefined && Number.isFinite(duration) && duration > 0) {
      this.duration = duration;
    }

    if (this.pageAdvance && this.pageRange) {
      const pageEnd = this.pageRange.end + this.delay;
      if (previousTime < pageEnd && seconds >= pageEnd) {
        if (this.isPlaying) this.pausePlayback();
        return;
      }
    }

    if (this.stopPlaybackTime !== null && seconds >= this.stopPlaybackTime) {
      this.stopPlaybackTime = null;
      if (this.isPlaying) this.pausePlayback();
    }

    const second = Math.floor(seconds);
    if (second !== this.lastUpdate) {
      this.lastUpdate = second;
      this.playback.lastPosition = seconds;
      this.savePlayback();
    }

    this.updateCue(seconds);
    this.updateChapter();
  }

  private seek(
    seconds: number,
    startPlayback = false,
    updateCue = true,
    stopPlaybackTime: number | null = null,
  ) {
    if (!this.audio) return;
    this.lastCue = null;
    this.cancelImagePause();
    if (this.pageRange && (seconds - this.delay < this.pageRange.start || seconds - this.delay > this.pageRange.end)) {
      this.pageRange = null;
    }
    this.audio.currentTime = seconds;
    this.stopPlaybackTime = stopPlaybackTime;
    if (updateCue) {
      this.tick(seconds);
    } else {
      this.currentTime = seconds;
    }
    if (startPlayback) this.startPlayback();
  }

  private savePlayback() {
    this.playback.delay = this.delay;
    this.playback.rate = this.rate;
    this.playback.volume = this.volume;
    invoke("sasayaki_save_playback", { id: this.id, playback: this.playback });
  }

  private updateCue(time: number) {
    if (!this.hasAudio || !this.hasMatch || this.chapterTransition || this.pausedOnImage) return;

    const cue = this.timeline.cue(time - this.delay);
    if (!cue) {
      this.clearDisplayedCue();
      return;
    }
    if (cue.id === this.currentCue?.id) return;

    if (
      this.isPlaying &&
      this.autoScroll &&
      this.hasPlayedOnce &&
      sasayakiConfig.sasayakiImagePause &&
      this.lastCue
    ) {
      const prev = this.lastCue;
      if (
        cue.chapterIndex > prev.chapterIndex ||
        (cue.chapterIndex === prev.chapterIndex && cue.start > prev.start)
      ) {
        const image = this.imageBetween(prev, cue);
        if (image) {
          this.beginImagePause(image, cue);
          return;
        }
      }
    }

    const currentIndex = this.getCurrentIndex();
    if (cue.chapterIndex === currentIndex) {
      this.displayCue(cue, this.autoScroll && this.hasPlayedOnce);
    } else if (this.autoScroll && this.hasPlayedOnce) {
      this.currentCue = cue;
      this.pendingCue = cue;
      this.loadChapter(cue.chapterIndex);
    } else {
      this.clearDisplayedCue();
    }
  }

  private displayCue(cue: SasayakiMatch, reveal: boolean) {
    this.currentCue = cue;
    this.lastCue = cue;
    this.bridge.highlightCue(cue.id, reveal);
  }

  private clearDisplayedCue() {
    if (!this.currentCue) return;
    this.currentCue = null;
    this.bridge.clearCue();
  }

  private imageBetween(prev: SasayakiMatch, next: SasayakiMatch): SasayakiImage | null {
    const after = (image: SasayakiImage) =>
      image.chapterIndex > prev.chapterIndex ||
      (image.chapterIndex === prev.chapterIndex && image.offset >= prev.start + prev.length);
    const before = (image: SasayakiImage) =>
      image.chapterIndex < next.chapterIndex ||
      (image.chapterIndex === next.chapterIndex && image.offset <= next.start);
    return (this.matchData?.images ?? []).find((image) => after(image) && before(image)) ?? null;
  }

  private beginImagePause(image: SasayakiImage, resume: SasayakiMatch) {
    this.pausedOnImage = true;
    this.imageResumeCue = resume;
    this.pausePlayback();
    if (image.chapterIndex === this.getCurrentIndex()) {
      this.scrollAndPause(image);
    } else {
      this.pendingImage = image;
      this.loadChapter(image.chapterIndex);
    }
  }

  private scrollAndPause(image: SasayakiImage) {
    this.bridge.scrollToImage(image.imageIndex);
  }

  private finishImagePause() {
    clearTimeout(this.imagePauseTimer);
    this.imagePauseTimer = 0;
    this.pendingImage = null;
    this.pausedOnImage = false;
    const resume = this.imageResumeCue;
    if (!resume) {
      this.startPlayback();
      return;
    }
    this.imageResumeCue = null;
    if (resume.chapterIndex === this.getCurrentIndex()) {
      this.displayCue(resume, this.autoScroll && this.hasPlayedOnce);
      this.startPlayback();
    } else {
      this.currentCue = resume;
      this.pendingCue = resume;
      this.startPlayback();
      this.loadChapter(resume.chapterIndex);
    }
  }

  private cancelImagePause() {
    if (!this.pausedOnImage) return;
    clearTimeout(this.imagePauseTimer);
    this.imagePauseTimer = 0;
    this.pausedOnImage = false;
    this.pendingImage = null;
    this.imageResumeCue = null;
    this.lastCue = null;
  }
}
