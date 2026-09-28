import { persisted } from "./persisted.svelte";

export type StatisticsAutostartMode = "Off" | "Page Turn" | "On";
export type StatisticsGoalMetric = "time" | "characters";

export type StatsConfig = {
  statisticsAutostartMode: StatisticsAutostartMode;
  statisticsResetTime: number;
  statisticsGoalMetric: StatisticsGoalMetric;
  statisticsDailyTimeGoal: number;
  statisticsDailyCharacterGoal: number;
  statisticsHideOnHome: boolean;
};

const defaults: StatsConfig = {
  statisticsAutostartMode: "Off",
  statisticsResetTime: 0,
  statisticsGoalMetric: "time",
  statisticsDailyTimeGoal: 20,
  statisticsDailyCharacterGoal: 5000,
  statisticsHideOnHome: false,
};

const store = persisted<StatsConfig>("stats.config", defaults);

export const statsConfig = store.config;
export const saveStatsConfig = store.save;
