<script lang="ts">
  import SettingRow from "./SettingRow.svelte";

  let { label, value = $bindable(), display, min, max, step, compact = false, commit = false, onchange }: {
    label: string;
    value: number;
    display?: string;
    min: number;
    max: number;
    step: number;
    compact?: boolean;
    commit?: boolean;
    onchange: (value: number) => void;
  } = $props();
</script>

<div class="flex flex-col gap-1.5">
  <SettingRow {label} {compact}>
    <span class="text-sm font-medium tabular-nums">{display ?? value}</span>
  </SettingRow>
  <input
    type="range"
    class="range range-xs text-primary"
    {min}
    {max}
    {step}
    {value}
    oninput={(e) => {
      value = Number(e.currentTarget.value);
      if (!commit) onchange(value);
    }}
    onchange={() => commit && onchange(value)}
  />
</div>
