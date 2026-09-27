import { invoke } from "@tauri-apps/api/core";
import { dayKey } from "./statsModel.svelte";
import type { ReadingDay, ReadingSession, Sessions } from "./types";

function startingSession(): ReadingSession {
  const timestamp = Date.now();
  return { startedAt: timestamp, endedAt: timestamp, charactersRead: 0, readingTime: 0 };
}

function hasActivity(session: ReadingSession) {
  return session.charactersRead > 0 || session.readingTime > 0;
}

export function statisticsDay(milliseconds: number, resetTime: number) {
  return dayKey(new Date(milliseconds - resetTime * 60000));
}

export class StatsTracker {
  isTracking = $state(false);
  isPaused = false;
  currentSession = $state<ReadingSession>(startingSession());
  private sessionId = crypto.randomUUID().toUpperCase();
  private history = $state.raw<Sessions>({});
  private lastTimestamp = Date.now();
  private lastCount = 0;

  constructor(
    private folder: string,
    private resetTime: () => number,
    private currentCharacter: () => number,
    private applyingBookmark: () => boolean,
  ) {}

  private historyDays = $derived.by(() => {
    const days: Record<string, ReadingDay> = {};
    for (const [id, change] of Object.entries(this.history)) {
      if (id === this.sessionId || !change.value) continue;
      const date = statisticsDay(change.value.startedAt, this.resetTime());
      days[date] ??= { dateKey: date, charactersRead: 0, readingTime: 0 };
      days[date].charactersRead += change.value.charactersRead;
      days[date].readingTime += change.value.readingTime;
    }
    return days;
  });

  private historyTotal = $derived(
    Object.values(this.historyDays).reduce(
      (total, day) => ({
        dateKey: total.dateKey,
        charactersRead: total.charactersRead + day.charactersRead,
        readingTime: total.readingTime + day.readingTime,
      }),
      { dateKey: "", charactersRead: 0, readingTime: 0 },
    ),
  );

  todaysTotal = $derived.by(() => {
    const today = statisticsDay(Date.now(), this.resetTime());
    const total = { ...(this.historyDays[today] ?? { dateKey: today, charactersRead: 0, readingTime: 0 }) };
    if (statisticsDay(this.currentSession.startedAt, this.resetTime()) === today) {
      total.charactersRead += this.currentSession.charactersRead;
      total.readingTime += this.currentSession.readingTime;
    }
    return total;
  });

  allTimeTotal = $derived({
    dateKey: "",
    charactersRead: this.historyTotal.charactersRead + this.currentSession.charactersRead,
    readingTime: this.historyTotal.readingTime + this.currentSession.readingTime,
  });

  async loadSessions() {
    this.applySessions(await invoke<Sessions>("load_statistics", { folder: this.folder }));
  }

  startTracking() {
    if (!hasActivity(this.currentSession)) this.currentSession = startingSession();
    this.isTracking = true;
    this.resetTrackingBaseline();
  }

  async stopTracking() {
    if (!this.isTracking) return;
    const saving = this.flushStats();
    this.isTracking = false;
    await saving;
  }

  updateStats() {
    if (this.applyingBookmark()) return;
    const now = Date.now();
    const timeDiff = (now - this.lastTimestamp) / 1000;
    const charDiff = Math.max(this.currentCharacter() - this.lastCount, -this.currentSession.charactersRead);
    if (timeDiff <= 0) return;

    this.currentSession = {
      ...this.currentSession,
      charactersRead: Math.max(this.currentSession.charactersRead + charDiff, 0),
      readingTime: this.currentSession.readingTime + timeDiff,
      endedAt: now,
    };

    this.lastTimestamp = now;
    this.lastCount = this.currentCharacter();
  }

  resetTrackingBaseline() {
    this.lastCount = this.currentCharacter();
    this.lastTimestamp = Date.now();
  }

  async pause() {
    const saving = this.flushStats();
    this.isPaused = true;
    await saving;
  }

  resume() {
    this.isPaused = false;
    this.resetTrackingBaseline();
  }

  flushStats() {
    if (!this.isTracking) return;
    if (!this.isPaused) this.updateStats();
    return this.saveStats();
  }

  applySessions(sessions: Sessions) {
    const change = sessions[this.sessionId];
    if (change && change.value === null) {
      this.sessionId = crypto.randomUUID().toUpperCase();
      this.currentSession = startingSession();
    }
    this.history = sessions;
  }

  private async saveStats() {
    this.applySessions(
      await invoke<Sessions>("save_reading_session", {
        folder: this.folder,
        sessionId: this.sessionId,
        session: $state.snapshot(this.currentSession),
      }),
    );
  }
}
