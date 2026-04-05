<script lang="ts">
  import { api, type CatalogModel, type DownloadProgressEvent } from '../api';
  import { listen } from '@tauri-apps/api/event';

  interface Props {
    onDismiss: () => void;
  }
  let { onDismiss }: Props = $props();

  let step = $state<'welcome' | 'pick' | 'downloading'>('welcome');
  let catalog = $state<CatalogModel[]>([]);
  let selectedModel = $state<string | null>(null);
  let downloadBytes = $state(0);
  let downloadTotal = $state(1);
  let error = $state('');

  const progressPercent = $derived(Math.round((downloadBytes / downloadTotal) * 100));

  const recommendedModels = $derived(
    catalog.filter(m => m.recommended || m.param_label === '1.5B' || m.param_label === '360M')
      .slice(0, 3)
  );

  async function loadCatalog() {
    try {
      catalog = await api.getModelCatalog();
    } catch (err) {
      error = `Failed to load models: ${err}`;
    }
  }

  async function startDownload() {
    if (!selectedModel) return;
    const model = catalog.find(m => m.id === selectedModel);
    if (!model) return;

    step = 'downloading';
    downloadTotal = model.size_bytes;
    downloadBytes = 0;
    error = '';

    try {
      await api.downloadModel(selectedModel);
      await api.activateModel(selectedModel);
      onDismiss();
    } catch (err) {
      error = `Download failed: ${err}`;
      step = 'pick';
    }
  }

  function handleSkip() {
    localStorage.setItem('grymoire-welcome-dismissed', '1');
    onDismiss();
  }

  $effect(() => {
    const unlisten = listen<DownloadProgressEvent>('llm-download-progress', (event) => {
      if (event.payload.phase === 'downloading') {
        downloadBytes = event.payload.bytes_downloaded;
        downloadTotal = event.payload.total_bytes;
      }
    });
    return () => { unlisten.then(fn => fn()); };
  });

  function formatBytes(bytes: number): string {
    if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
    return `${Math.round(bytes / 1_000_000)} MB`;
  }

  loadCatalog();
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
  role="dialog"
  aria-modal="true"
  aria-label="Welcome to Grymoire"
  tabindex="-1"
  onkeydown={(e) => { if (e.key === 'Escape' && step !== 'downloading') handleSkip(); }}>
  <div class="bg-surface rounded-xl shadow-2xl border border-border w-full max-w-lg mx-4 overflow-hidden">

    {#if step === 'welcome'}
      <!-- Welcome -->
      <div class="p-8 text-center">
        <div class="w-14 h-14 rounded-2xl bg-accent/10 flex items-center justify-center mx-auto mb-4">
          <svg class="w-7 h-7 text-accent" viewBox="0 0 48 46" fill="currentColor">
            <path d="M25.946 44.938c-.664.845-2.021.375-2.021-.698V33.937a2.26 2.26 0 0 0-2.262-2.262H10.287c-.92 0-1.456-1.04-.92-1.788l7.48-10.471c1.07-1.497 0-3.578-1.842-3.578H1.237c-.92 0-1.456-1.04-.92-1.788L10.013.474c.214-.297.556-.474.92-.474h28.894c.92 0 1.456 1.04.92 1.788l-7.48 10.471c-1.07 1.498 0 3.579 1.842 3.579h11.377c.943 0 1.473 1.088.89 1.83L25.947 44.94z"/>
          </svg>
        </div>
        <h2 class="text-[18px] font-semibold text-text-primary mb-2">Welcome to Grymoire</h2>
        <p class="text-[13px] text-text-secondary leading-relaxed max-w-sm mx-auto">
          Grymoire uses a local AI model to generate titles for your saved articles and PDFs.
          Choose a model to get started — everything runs on your machine, nothing leaves your device.
        </p>
        <div class="mt-6 flex flex-col gap-2">
          <button
            class="px-5 py-2.5 rounded-lg bg-accent text-white text-[13px] font-medium hover:bg-accent-hover transition-colors"
            onclick={() => { step = 'pick'; }}
          >Choose a Model</button>
          <button
            class="text-[12px] text-text-tertiary hover:text-text-secondary transition-colors"
            onclick={handleSkip}
          >Skip for now</button>
        </div>
      </div>

    {:else if step === 'pick'}
      <!-- Model Picker -->
      <div class="p-6">
        <h2 class="text-[16px] font-semibold text-text-primary mb-1">Choose a Model</h2>
        <p class="text-[12px] text-text-tertiary mb-4">
          Smaller models are faster but less capable. You can change this later in Settings.
        </p>

        <div class="space-y-2 mb-4">
          {#each recommendedModels as model (model.id)}
            <button
              class="w-full text-left p-3 rounded-lg border transition-all
                {selectedModel === model.id ? 'border-accent bg-accent-soft/50 shadow-sm' : 'border-border-subtle hover:border-accent/30 bg-surface-raised'}"
              onclick={() => { selectedModel = model.id; }}
            >
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-2">
                  <h4 class="text-[13px] font-semibold text-text-primary">{model.name}</h4>
                  {#if model.recommended}
                    <span class="text-[9px] px-1.5 py-0.5 rounded-md bg-accent-soft text-accent font-brand uppercase tracking-wider">Recommended</span>
                  {/if}
                </div>
                <span class="font-brand text-[11px] text-text-tertiary">{model.size_display}</span>
              </div>
              <p class="text-[11px] text-text-secondary mt-0.5">{model.description}</p>
              <div class="flex gap-3 mt-1.5 font-brand text-[10px] text-text-tertiary">
                <span>{model.param_label}</span>
                <span>{model.quant_label}</span>
                <span>~{(model.min_ram_mb / 1024).toFixed(1)} GB RAM</span>
              </div>
            </button>
          {/each}
        </div>

        {#if error}
          <p class="text-[12px] text-red-500 mb-3">{error}</p>
        {/if}

        <div class="flex items-center justify-between">
          <button
            class="text-[12px] text-text-tertiary hover:text-text-secondary transition-colors"
            onclick={handleSkip}
          >Skip for now</button>
          <button
            class="px-5 py-2 rounded-lg bg-accent text-white text-[13px] font-medium hover:bg-accent-hover transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
            onclick={startDownload}
            disabled={!selectedModel}
          >Download & Activate</button>
        </div>
      </div>

    {:else if step === 'downloading'}
      <!-- Download Progress -->
      {#if true}
        {@const model = catalog.find(m => m.id === selectedModel)}
        <div class="p-8">
          <div class="text-center mb-6">
            <h2 class="text-[16px] font-semibold text-text-primary mb-1">Downloading Model</h2>
            <p class="text-[12px] text-text-tertiary">{model?.name ?? selectedModel}</p>
          </div>

          <div class="w-full h-2 bg-surface-sunken rounded-full overflow-hidden mb-2">
            <div
              class="h-full bg-accent rounded-full transition-all duration-300"
              style="width: {progressPercent}%"
            ></div>
          </div>

          <div class="flex justify-between font-brand text-[11px] text-text-tertiary">
            <span>{formatBytes(downloadBytes)} / {model?.size_display ?? '?'}</span>
            <span>{progressPercent}%</span>
          </div>

          {#if error}
            <p class="text-[12px] text-red-500 mt-4 text-center">{error}</p>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>
