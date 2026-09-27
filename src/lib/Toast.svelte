<script lang="ts">
  let toast = $state<{ message: string; kind: "success" | "error" } | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;

  export function show(message: string, kind: "success" | "error" = "success") {
    toast = { message, kind };
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => (toast = null), 4000);
  }
</script>

{#if toast}
  <div class="toast toast-end z-50">
    <div class="alert {toast.kind === 'error' ? 'alert-error' : 'alert-success'}">
      <span class="select-text whitespace-pre-line">{toast.message}</span>
    </div>
  </div>
{/if}
