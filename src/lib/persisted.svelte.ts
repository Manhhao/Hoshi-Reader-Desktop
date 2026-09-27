export function persisted<T extends object>(key: string, defaults: T) {
  let initial: T = { ...defaults };
  try {
    const stored = localStorage.getItem(key);
    if (stored) initial = { ...defaults, ...JSON.parse(stored) };
  } catch {}
  const config = $state<T>(initial);
  return {
    config,
    save() {
      localStorage.setItem(key, JSON.stringify($state.snapshot(config)));
    },
  };
}
