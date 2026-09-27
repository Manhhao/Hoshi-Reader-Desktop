import { invoke } from "@tauri-apps/api/core";
import { statsConfig } from "./statsConfig.svelte";
import type { BookStatistics, ReadingDay, StatisticsPeriod } from "./types";

export const periods: StatisticsPeriod[] = ["Week", "Month", "Year", "All"];

export function firstWeekday(): number {
  const locale = new Intl.Locale(navigator.language) as Intl.Locale & {
    getWeekInfo?: () => { firstDay: number };
    weekInfo?: { firstDay: number };
  };
  const info = locale.getWeekInfo?.() ?? locale.weekInfo;
  return (info?.firstDay ?? 1) % 7;
}

export function dayKey(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

export function keyToDate(key: string): Date {
  const [year, month, day] = key.split("-").map(Number);
  return new Date(year, month - 1, day);
}

export function addDays(date: Date, count: number): Date {
  const next = new Date(date);
  next.setDate(next.getDate() + count);
  return next;
}

export function addMonths(date: Date, count: number): Date {
  return new Date(date.getFullYear(), date.getMonth() + count, 1);
}

export function startOfWeek(date: Date): Date {
  const first = firstWeekday();
  const start = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  start.setDate(start.getDate() - ((start.getDay() - first + 7) % 7));
  return start;
}

export function startOfMonth(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), 1);
}

export function startOfYear(date: Date): Date {
  return new Date(date.getFullYear(), 0, 1);
}

export function readingSpeed(day: { charactersRead: number; readingTime: number }): number {
  return day.readingTime > 0 ? Math.trunc((day.charactersRead / day.readingTime) * 3600) : 0;
}

export function formatUnits(seconds: number): string {
  const total = Math.round(seconds / 60);
  const hours = Math.trunc(total / 60);
  const minutes = total % 60;
  if (hours === 0) return `${minutes}m`;
  return minutes === 0 ? `${hours}h` : `${hours}h ${minutes}m`;
}

export function formatMinuteSecond(seconds: number): string {
  const total = Math.round(seconds);
  return `${Math.trunc(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

const number = new Intl.NumberFormat();
export function formatNumber(value: number): string {
  return number.format(value);
}

function emptyDay(key: string): ReadingDay {
  return { dateKey: key, charactersRead: 0, readingTime: 0 };
}

function sum(days: ReadingDay[], key: string): ReadingDay {
  return days.reduce(
    (total, day) => ({
      dateKey: key,
      charactersRead: total.charactersRead + day.charactersRead,
      readingTime: total.readingTime + day.readingTime,
    }),
    emptyDay(key),
  );
}

export class StatisticsModel {
  books = $state<BookStatistics[]>([]);
  period = $state<StatisticsPeriod>("Week");
  referenceDate = $state<Date>(new Date());
  selectedKey = $state<string | null>(null);
  visibleBookCount = $state(5);
  private now = $state(Date.now());
  private reloadTimer: ReturnType<typeof setTimeout> | undefined;

  today = $derived(
    dayKey(new Date(this.now - statsConfig.statisticsResetTime * 3600000)),
  );

  private byDay = $derived.by(() => {
    const grouped = new Map<string, ReadingDay>();
    for (const book of this.books) {
      for (const day of book.days) {
        const entry = grouped.get(day.dateKey) ?? emptyDay(day.dateKey);
        entry.charactersRead += day.charactersRead;
        entry.readingTime += day.readingTime;
        grouped.set(day.dateKey, entry);
      }
    }
    return grouped;
  });

  days = $derived([...this.byDay.values()].sort((a, b) => a.dateKey.localeCompare(b.dateKey)));

  private byMonth = $derived.by(() => {
    const grouped = new Map<string, ReadingDay>();
    for (const day of this.days) {
      const key = `${day.dateKey.slice(0, 7)}-01`;
      const entry = grouped.get(key) ?? emptyDay(key);
      entry.charactersRead += day.charactersRead;
      entry.readingTime += day.readingTime;
      grouped.set(key, entry);
    }
    return grouped;
  });

  firstDay = $derived(this.days[0]?.dateKey ?? null);

  todaysReading = $derived(this.byDay.get(this.today) ?? emptyDay(this.today));

  bucketUnit = $derived<"day" | "month">(
    this.period === "Week" || this.period === "Month" ? "day" : "month",
  );

  goal = $derived(
    statsConfig.statisticsGoalMetric === "time"
      ? statsConfig.statisticsDailyTimeGoal
      : statsConfig.statisticsDailyCharacterGoal,
  );

  goalDates = $derived(this.days.filter((day) => this.goalMet(day)).map((day) => day.dateKey));
  streaks = $derived(streaks(this.goalDates, this.today));

  metricValue(day: ReadingDay | undefined): number {
    if (!day) return 0;
    return statsConfig.statisticsGoalMetric === "time"
      ? day.readingTime / 60
      : day.charactersRead;
  }

  goalMet(day: ReadingDay | undefined): boolean {
    return this.metricValue(day) >= this.goal;
  }

  day(key: string): ReadingDay | undefined {
    return this.byDay.get(key);
  }

  periodStart(date: Date): Date | null {
    switch (this.period) {
      case "Week":
        return startOfWeek(date);
      case "Month":
        return startOfMonth(date);
      case "Year":
        return startOfYear(date);
      default:
        return null;
    }
  }

  periodEnd(date: Date): Date | null {
    const start = this.periodStart(date);
    if (!start) return null;
    switch (this.period) {
      case "Week":
        return addDays(start, 7);
      case "Month":
        return addMonths(start, 1);
      default:
        return new Date(start.getFullYear() + 1, 0, 1);
    }
  }

  previousDate(date: Date): Date | null {
    switch (this.period) {
      case "Week":
        return addDays(startOfWeek(date), -7);
      case "Month":
        return addMonths(startOfMonth(date), -1);
      case "Year":
        return new Date(date.getFullYear() - 1, 0, 1);
      default:
        return null;
    }
  }

  referenceDates = $derived.by(() => {
    const todayDate = keyToDate(this.today);
    if (this.period === "All" || !this.firstDay) return [todayDate];
    const first = keyToDate(this.firstDay);
    const dates: Date[] = [];
    let cursor = todayDate;
    while (true) {
      dates.push(cursor);
      const previous = this.previousDate(cursor);
      if (!previous || (this.periodEnd(previous) ?? previous) <= first) break;
      cursor = previous;
    }
    return dates.reverse();
  });

  buckets(date: Date): ReadingDay[] {
    const unit = this.bucketUnit;
    const start = this.periodStart(date) ?? keyToDate(this.firstDay ?? this.today);
    const end = this.periodEnd(date) ?? addDays(keyToDate(this.today), 1);
    const grouped = unit === "day" ? this.byDay : this.byMonth;
    const buckets: ReadingDay[] = [];
    let cursor = unit === "day" ? start : startOfMonth(start);
    while (cursor < end) {
      const key = dayKey(cursor);
      buckets.push(grouped.get(key) ?? emptyDay(key));
      cursor = unit === "day" ? addDays(cursor, 1) : addMonths(cursor, 1);
    }
    return buckets;
  }

  private periodDays(date: Date): ReadingDay[] {
    const start = this.periodStart(date);
    if (!start) return this.days;
    const from = dayKey(start);
    const to = dayKey(this.periodEnd(date)!);
    return this.days.filter((day) => day.dateKey >= from && day.dateKey < to);
  }

  selectedDay = $derived(
    this.selectedKey === null
      ? null
      : (this.buckets(this.referenceDate).find((day) => day.dateKey === this.selectedKey) ?? null),
  );

  summary = $derived(
    this.selectedDay ?? sum(this.periodDays(this.referenceDate), this.today),
  );

  averageReadingTime(date: Date): number | null {
    const elapsed = this.buckets(date).filter((day) => day.dateKey <= this.today).length;
    if (elapsed === 0) return null;
    return this.periodDays(date).reduce((total, day) => total + day.readingTime, 0) / elapsed;
  }

  readingTimeDelta = $derived.by(() => {
    if (this.period === "All" || this.selectedKey !== null) return null;
    const previous = this.previousDate(this.referenceDate);
    if (!previous) return null;
    const current = this.averageReadingTime(this.referenceDate);
    const earlier = this.averageReadingTime(previous);
    if (current === null || earlier === null || earlier <= 0) return null;
    return ((current - earlier) / earlier) * 100;
  });

  periodBooks = $derived.by(() => {
    const start = this.selectedKey
      ? this.selectedKey
      : dayKey(this.periodStart(this.referenceDate) ?? keyToDate(this.firstDay ?? this.today));
    const end = this.selectedKey
      ? dayKey(
          this.bucketUnit === "day"
            ? addDays(keyToDate(this.selectedKey), 1)
            : addMonths(keyToDate(this.selectedKey), 1),
        )
      : dayKey(this.periodEnd(this.referenceDate) ?? addDays(keyToDate(this.today), 1));
    return this.books
      .map((book) => ({
        ...book,
        days: book.days.filter((day) => day.dateKey >= start && day.dateKey < end),
      }))
      .filter((book) => book.days.length > 0)
      .map((book) => ({
        ...book,
        readingTime: book.days.reduce((total, day) => total + day.readingTime, 0),
        charactersRead: book.days.reduce((total, day) => total + day.charactersRead, 0),
      }))
      .sort((a, b) => b.readingTime - a.readingTime);
  });

  setPeriod(period: StatisticsPeriod) {
    this.period = period;
    this.referenceDate = keyToDate(this.today);
    this.selectedKey = null;
    this.visibleBookCount = 5;
  }

  setReferenceDate(date: Date) {
    this.referenceDate = date;
    this.selectedKey = null;
    this.visibleBookCount = 5;
  }

  select(key: string | null) {
    this.selectedKey = this.selectedKey === key ? null : key;
    this.visibleBookCount = 5;
  }

  refreshDate() {
    const previous = this.today;
    this.now = Date.now();
    if (this.today !== previous && dayKey(this.referenceDate) === previous) {
      this.referenceDate = keyToDate(this.today);
    }
  }

  async load() {
    this.books = await invoke<BookStatistics[]>("load_all_statistics", {
      resetTime: Math.round(statsConfig.statisticsResetTime * 60),
    });
    this.refreshDate();
    this.referenceDate = keyToDate(this.today);
  }

  scheduleLoad() {
    if (this.reloadTimer) return;
    this.reloadTimer = setTimeout(() => {
      this.reloadTimer = undefined;
      this.load();
    }, 2000);
  }
}

function streaks(metKeys: string[], todayKey: string) {
  let current = { count: 0, from: "", to: "" };
  let longest = { count: 0, from: "", to: "" };
  for (const key of metKeys) {
    const previous = dayKey(addDays(keyToDate(key), -1));
    current =
      current.count > 0 && current.to === previous
        ? { count: current.count + 1, from: current.from, to: key }
        : { count: 1, from: key, to: key };
    if (current.count > longest.count) longest = { ...current };
  }
  const yesterday = dayKey(addDays(keyToDate(todayKey), -1));
  if (current.count === 0 || (current.to !== todayKey && current.to !== yesterday)) {
    return { current: { count: 0, from: "", to: "" }, longest };
  }
  return { current, longest };
}
