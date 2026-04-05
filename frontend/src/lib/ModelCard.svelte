<script lang="ts">
  import type { CatalogModel } from '../api';

  interface Props {
    model: CatalogModel;
    isActive: boolean;
    progress?: { bytes: number; total: number; phase: string };
    onDownload: () => void;
    onDelete: () => void;
    onActivate: () => void;
  }
  let { model, isActive, progress, onDownload, onDelete, onActivate }: Props = $props();

  const progressPercent = $derived(
    progress ? Math.round((progress.bytes / progress.total) * 100) : 0
  );

  const isDownloading = $derived(progress?.phase === 'downloading');

  function formatBytes(bytes: number): string {
    if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
    return `${Math.round(bytes / 1_000_000)} MB`;
  }
</script>

<div class="rounded-lg bg-surface-raised border transition-all
  {isActive ? 'border-accent shadow-sm shadow-accent/10' : 'border-border-subtle'}">
  <div class="p-3">
    <!-- Header row -->
    <div class="flex items-start justify-between gap-2">
      <div class="flex-1 min-w-0">
        <div class="flex items-center gap-2">
          <h4 class="text-[13px] font-semibold text-text-primary">{model.name}</h4>
          {#if model.recommended}
            <span class="text-[9px] px-1.5 py-0.5 rounded-md bg-accent-soft text-accent font-brand uppercase tracking-wider">Recommended</span>
          {/if}
          {#if isActive}
            <span class="text-[9px] px-1.5 py-0.5 rounded-md bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 font-brand">Active</span>
          {/if}
        </div>
        <p class="text-[11px] text-text-secondary mt-0.5">{model.description}</p>
      </div>
    </div>

    <!-- Stats row -->
    <div class="flex items-center gap-3 mt-2 font-brand text-[10px] text-text-tertiary">
      <span>{model.param_label}</span>
      <span>{model.quant_label}</span>
      <span>{model.size_display}</span>
      <span>{(model.context_size / 1024).toFixed(0)}K ctx</span>
      <span>~{(model.min_ram_mb / 1024).toFixed(1)} GB RAM</span>
    </div>

    <!-- Progress bar -->
    {#if isDownloading}
      <div class="mt-3">
        <div class="w-full h-1.5 bg-surface-sunken rounded-full overflow-hidden">
          <div
            class="h-full bg-accent rounded-full transition-all duration-300"
            style="width: {progressPercent}%"
          ></div>
        </div>
        <div class="flex justify-between mt-1 font-brand text-[9px] text-text-tertiary">
          <span>{formatBytes(progress?.bytes ?? 0)} / {model.size_display}</span>
          <span>{progressPercent}%</span>
        </div>
      </div>
    {/if}

    <!-- Actions -->
    <div class="flex items-center gap-2 mt-2.5">
      {#if isDownloading}
        <span class="text-[11px] text-text-tertiary">Downloading...</span>
      {:else if !model.downloaded}
        <button
          class="text-[11px] px-3 py-1 rounded-md bg-accent text-white hover:bg-accent-hover transition-colors"
          onclick={onDownload}
        >Download</button>
      {:else if !isActive}
        <button
          class="text-[11px] px-3 py-1 rounded-md border border-accent text-accent hover:bg-accent hover:text-white transition-colors"
          onclick={onActivate}
        >Activate</button>
        <button
          class="text-[11px] px-2.5 py-1 rounded-md border border-border text-text-tertiary hover:text-red-500 hover:border-red-300 dark:hover:border-red-800 transition-colors"
          onclick={onDelete}
        >Delete</button>
      {:else}
        <span class="text-[11px] text-green-600 dark:text-green-400 font-brand flex items-center gap-1">
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
            <path d="M5 13l4 4L19 7" />
          </svg>
          Active
        </span>
        <button
          class="text-[11px] px-2.5 py-1 rounded-md border border-border text-text-tertiary hover:text-red-500 hover:border-red-300 dark:hover:border-red-800 transition-colors ml-auto"
          onclick={onDelete}
        >Delete</button>
      {/if}
    </div>
  </div>
</div>
