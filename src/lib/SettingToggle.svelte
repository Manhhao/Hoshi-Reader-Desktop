<script lang="ts">
  import SettingRow from "./SettingRow.svelte";

  let { label, checked = $bindable(), compact = false, onchange }: {
    label: string;
    checked: boolean;
    compact?: boolean;
    onchange: () => void;
  } = $props();
</script>

<SettingRow {label} {compact}>
  <input
    type="checkbox"
    class="toggle toggle-primary toggle-sm"
    {checked}
    onchange={(e) => {
      checked = e.currentTarget.checked;
      onchange();
    }}
  />
</SettingRow>
