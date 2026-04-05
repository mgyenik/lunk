<script lang="ts">
  import { api, type CatalogModel, type LlmStatus, type DownloadProgressEvent } from '../api';
  import { listen } from '@tauri-apps/api/event';
  import ModelCard from './ModelCard.svelte';

  let catalog = $state<CatalogModel[]>([]);
  let status = $state<LlmStatus | null>(null);
  let isLoading = $state(true);
  let error = $state('');
  let downloadProgress = $state<Record<string, { bytes: number; total: number; phase: string }>>({});

  async function loadData() {
    isLoading = true;
    error = '';
    try {
      const [cat, st] = await Promise.all([
        api.getModelCatalog(),
        api.getLlmStatus(),
      ]);
      catalog = cat;
      status = st;
    } catch (err) {
      error = `Failed to load settings: ${err}`;
    } finally {
      isLoading = false;
    }
  }

  async function handleDownload(modelId: string) {
    error = '';
    downloadProgress[modelId] = { bytes: 0, total: 1, phase: 'downloading' };
    try {
      await api.downloadModel(modelId);
      await loadData();
    } catch (err) {
      error = `Download failed: ${err}`;
      delete downloadProgress[modelId];
    }
  }

  async function handleDelete(modelId: string) {
    error = '';
    try {
      await api.deleteModel(modelId);
      await loadData();
    } catch (err) {
      error = `Delete failed: ${err}`;
    }
  }

  async function handleActivate(modelId: string) {
    error = '';
    try {
      await api.activateModel(modelId);
      await loadData();
    } catch (err) {
      error = `Activate failed: ${err}`;
    }
  }

  async function handleToggleTitleGen() {
    if (!status) return;
    error = '';
    try {
      await api.setTitleGeneration(!status.title_generation_enabled);
      status = await api.getLlmStatus();
    } catch (err) {
      error = `Failed to update setting: ${err}`;
    }
  }

  $effect(() => {
    const unlisten = listen<DownloadProgressEvent>('llm-download-progress', (event) => {
      const p = event.payload;
      downloadProgress[p.model_id] = {
        bytes: p.bytes_downloaded,
        total: p.total_bytes,
        phase: p.phase,
      };
      if (p.phase === 'complete') {
        // Slight delay to show 100% before refreshing
        setTimeout(() => {
          delete downloadProgress[p.model_id];
          loadData();
        }, 500);
      }
    });
    return () => { unlisten.then(fn => fn()); };
  });

  loadData();
</script>

<div class="flex-1 overflow-y-auto p-6 max-w-2xl">
  <h2 class="text-lg font-semibold text-text-primary mb-1">Settings</h2>
  <p class="text-body text-text-tertiary mb-6">Manage AI models and preferences</p>

  {#if isLoading}
    <div class="flex items-center justify-center py-20">
      <div class="w-5 h-5 rounded-full border-2 border-accent/20 border-t-accent animate-spin"></div>
    </div>
  {:else}
    <!-- LLM Status -->
    <div class="rounded-lg bg-surface-raised border border-border p-4 mb-6">
      <div class="flex items-center justify-between mb-2">
        <h3 class="text-base font-semibold text-text-primary">AI Model</h3>
        {#if status?.model_loaded}
          <span class="text-sm px-2 py-0.5 rounded-full bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 font-brand">
            Loaded
          </span>
        {:else}
          <span class="text-sm px-2 py-0.5 rounded-full bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400 font-brand">
            No model
          </span>
        {/if}
      </div>
      {#if status?.active_model}
        {@const activeEntry = catalog.find(m => m.id === status?.active_model)}
        {#if activeEntry}
          <p class="text-body text-text-secondary">
            {activeEntry.name} <span class="text-text-tertiary">({activeEntry.param_label} {activeEntry.quant_label})</span>
          </p>
        {/if}
      {:else}
        <p class="text-body text-text-secondary">
          Download and activate a model to enable AI title generation.
        </p>
      {/if}
    </div>

    <!-- Title Generation Toggle -->
    <div class="rounded-lg bg-surface-raised border border-border p-4 mb-6">
      <div class="flex items-center justify-between">
        <div>
          <h3 class="text-base font-semibold text-text-primary">Title Generation</h3>
          <p class="text-ui text-text-tertiary mt-0.5">Use AI to generate titles for saved articles and PDFs</p>
        </div>
        <button
          class="relative w-10 h-5 rounded-full transition-colors duration-200 {status?.title_generation_enabled ? 'bg-accent' : 'bg-border'}"
          onclick={handleToggleTitleGen}
          role="switch"
          aria-checked={status?.title_generation_enabled ?? false}
          aria-label="Toggle title generation"
        >
          <span class="absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full shadow-sm transition-transform duration-200 {status?.title_generation_enabled ? 'translate-x-5' : ''}"></span>
        </button>
      </div>
    </div>

    <!-- Model Catalog -->
    <div class="mb-6">
      <h3 class="text-base font-semibold text-text-primary mb-3">Available Models</h3>
      <div class="space-y-2">
        {#each catalog as model (model.id)}
          <ModelCard
            {model}
            isActive={status?.active_model === model.id}
            progress={downloadProgress[model.id]}
            onDownload={() => handleDownload(model.id)}
            onDelete={() => handleDelete(model.id)}
            onActivate={() => handleActivate(model.id)}
          />
        {/each}
      </div>
    </div>
  {/if}

  {#if error}
    <div class="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-3 text-body text-red-700 dark:text-red-400">
      {error}
    </div>
  {/if}
</div>
