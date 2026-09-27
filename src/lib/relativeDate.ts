const units: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 31536000],
  ["month", 2592000],
  ["week", 604800],
  ["day", 86400],
  ["hour", 3600],
  ["minute", 60],
  ["second", 1],
];
const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

export function relativeDate(date: number) {
  const seconds = date + 978307200 - Date.now() / 1000;
  const [unit, size] = units.find(([, size]) => Math.abs(seconds) >= size) ?? units[units.length - 1];
  return formatter.format(Math.round(seconds / size), unit);
}
