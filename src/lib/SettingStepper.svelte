<script lang="ts">
  import { Minus, Plus } from "@lucide/svelte";
  import SettingRow from "./SettingRow.svelte";

  let { label, value = $bindable(), min, max, suffix = "", compact = false, onchange }: {
    label: string;
    value: number;
    min: number;
    max: number;
    suffix?: string;
    compact?: boolean;
    onchange: () => void;
  } = $props();

  function step(delta: number) {
    value = Math.min(max, Math.max(min, value + delta));
    onchange();
  }
</script>

<SettingRow {label} {compact}>
  <div class="flex items-center gap-2">
    <span class="text-right text-sm font-semibold tabular-nums" class:w-10={!compact}>{value}{suffix}</span>
    <div class="join">
      <button class="btn btn-sm btn-square join-item" onclick={() => step(-1)}>
        <Minus class="size-4" />
      </button>
      <button class="btn btn-sm btn-square join-item" onclick={() => step(1)}>
        <Plus class="size-4" />
      </button>
    </div>
  </div>
</SettingRow>
