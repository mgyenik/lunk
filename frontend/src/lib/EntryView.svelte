<script lang="ts">
  import { api, decodeBase64, formatDate, type Entry, type EntryContent, type SimilarEntry, type Keyword } from '../api';
  import FilterChip from './FilterChip.svelte';
  import PdfView from './PdfView.svelte';

  interface Props {
    entry: Entry;
    initialPage?: number;
    onBack: () => void;
    onTagsChange: (id: string, tags: string[]) => void;
    onDelete: (id: string) => void;
    onNavigate?: (entry: Entry) => void;
    onTagClick?: (tag: string) => void;
    onDomainClick?: (domain: string) => void;
    onKeywordClick?: (keyword: string) => void;
  }
  let { entry, initialPage, onBack, onTagsChange, onDelete, onNavigate, onTagClick, onDomainClick, onKeywordClick }: Props = $props();

  let content = $state<EntryContent | null>(null);
  let viewMode = $state<'archive' | 'reader'>('archive');
  let isLoading = $state(true);
  let loadError = $state('');
  let confirmingDelete = $state(false);
  let iframeEl: HTMLIFrameElement | undefined = $state();
  let tagInput = $state('');
  let similarEntries = $state<SimilarEntry[]>([]);
  let entryKeywords = $state<Keyword[]>([]);

  $effect(() => {
    loadContent(entry.id);
    loadSemantic(entry.id);
  });

  async function loadContent(id: string) {
    isLoading = true;
    loadError = '';
    try {
      content = await api.getEntryContent(id);
      viewMode = content.snapshot_html ? 'archive' : 'reader';
    } catch (err) {
      loadError = `Failed to load content: ${err}`;
    } finally {
      isLoading = false;
    }
  }

  $effect(() => {
    if (viewMode === 'archive' && content?.snapshot_html && iframeEl) {
      const html = decodeBase64(content.snapshot_html);
      const blob = new Blob([html], { type: 'text/html' });
      iframeEl.src = URL.createObjectURL(blob);
    }
  });

  async function loadSemantic(id: string) {
    // These may fail if the entry hasn't been embedded yet (pre-backfill)
    const [similar, kw] = await Promise.all([
      api.getSimilarEntries(id, 5).catch((): SimilarEntry[] => []),
      api.getEntryKeywords(id).catch((): Keyword[] => []),
    ]);
    similarEntries = similar;
    entryKeywords = kw;
  }

  function handleDelete() {
    if (confirmingDelete) {
      onDelete(entry.id);
      confirmingDelete = false;
    } else {
      confirmingDelete = true;
      setTimeout(() => { confirmingDelete = false; }, 3000);
    }
  }

  function addTag() {
    const tag = tagInput.trim().toLowerCase();
    if (tag && !entry.tags.includes(tag)) {
      onTagsChange(entry.id, [...entry.tags, tag]);
    }
    tagInput = '';
  }

  function removeTag(tag: string) {
    onTagsChange(entry.id, entry.tags.filter(t => t !== tag));
  }

  function handleTagKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') { e.preventDefault(); addTag(); }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (e.key === 'Escape') onBack();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex-1 flex flex-col min-h-0 bg-surface">
  <!-- Toolbar -->
  <div class="flex items-center gap-2 px-4 py-2 border-b border-border shrink-0">
    <button class="text-base text-text-secondary hover:text-accent flex items-center gap-1 transition-colors" onclick={onBack}>
      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path d="M15 19l-7-7 7-7" />
      </svg>
      <span class="font-brand text-ui">ESC</span>
    </button>

    <div class="flex-1"></div>

    {#if entry.content_type === 'article' && content?.snapshot_html}
      <div class="flex rounded-md border border-border text-ui overflow-hidden">
        <button
          class="px-2.5 py-1 transition-colors {viewMode === 'archive' ? 'bg-accent text-white' : 'text-text-secondary hover:bg-surface-raised'}"
          onclick={() => viewMode = 'archive'}
        >Archive</button>
        <button
          class="px-2.5 py-1 border-l border-border transition-colors {viewMode === 'reader' ? 'bg-accent text-white' : 'text-text-secondary hover:bg-surface-raised'}"
          onclick={() => viewMode = 'reader'}
        >Reader</button>
      </div>
    {/if}

    {#if entry.url}
      <a href={entry.url} target="_blank" rel="noopener noreferrer"
        class="text-ui px-2.5 py-1 rounded-md border border-border text-text-secondary hover:bg-surface-raised transition-colors">
        Original
      </a>
    {/if}

    <button
      class="text-ui px-2.5 py-1 rounded-md border transition-all duration-200
        {confirmingDelete ? 'border-red-400 bg-red-500 text-white hover:bg-red-600' : 'border-border text-text-tertiary hover:text-red-500 hover:border-red-300 dark:hover:border-red-800'}"
      onclick={handleDelete}
    >{confirmingDelete ? 'Confirm?' : 'Delete'}</button>
  </div>

  <!-- Entry header -->
  <div class="px-6 py-4 border-b border-border-subtle bg-surface-sunken shrink-0">
    <h1 class="text-lg font-semibold text-text-primary leading-tight">{entry.title}</h1>

    <!-- Metadata row: domain (clickable), date, counts -->
    <div class="flex items-center gap-3 mt-2 font-brand text-ui text-text-tertiary">
      {#if entry.domain}
        <FilterChip label={entry.domain} variant="domain" onclick={() => onDomainClick?.(entry.domain!)} />
      {/if}
      <span>{formatDate(entry.created_at)}</span>
      {#if entry.word_count}<span>{entry.word_count.toLocaleString()}w</span>{/if}
      {#if entry.page_count}<span>{entry.page_count}pg</span>{/if}
    </div>

    <!-- Tags — clickable to filter, with add/remove -->
    <div class="flex items-center gap-1.5 mt-2.5 flex-wrap">
      {#each entry.tags as tag}
        <span class="inline-flex items-center gap-0.5">
          <FilterChip label={tag} variant="tag" onclick={() => onTagClick?.(tag)} />
          <button class="text-accent/40 hover:text-accent text-sm transition-opacity" onclick={() => removeTag(tag)}>&times;</button>
        </span>
      {/each}
      <input
        type="text"
        bind:value={tagInput}
        onkeydown={handleTagKeydown}
        placeholder="+ tag"
        class="text-ui px-1.5 py-[2px] w-14 bg-transparent border-b border-transparent focus:border-accent/30 outline-none text-text-secondary placeholder-text-tertiary"
      />
    </div>

    <!-- Keywords — clickable to search -->
    {#if entryKeywords.length > 0}
      <div class="flex items-center gap-1.5 mt-2 flex-wrap">
        {#each entryKeywords.slice(0, 6) as kw}
          <FilterChip label={kw.keyword} variant="keyword" onclick={() => onKeywordClick?.(kw.keyword)} />
        {/each}
      </div>
    {/if}
  </div>

  <!-- Related entries -->
  {#if similarEntries.length > 0}
    <div class="px-6 py-2.5 border-b border-border-subtle bg-surface shrink-0">
      <h3 class="text-sm font-semibold uppercase tracking-[0.1em] text-text-tertiary mb-2">Related</h3>
      <div class="flex gap-2 overflow-x-auto scroll-x-hidden pb-1">
        {#each similarEntries as sim}
          <button
            class="shrink-0 px-2.5 py-1.5 rounded-md bg-surface-raised border border-border-subtle text-left
              hover:border-accent/30 transition-all text-ui max-w-[200px]"
            onclick={() => onNavigate?.(sim)}
          >
            <p class="font-medium text-text-primary truncate">{sim.title}</p>
            <span class="font-brand text-xs text-text-tertiary">{Math.round(sim.similarity * 100)}% match</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Content -->
  <div class="flex-1 min-h-0 overflow-hidden">
    {#if isLoading}
      <div class="flex items-center justify-center h-full">
        <div class="w-5 h-5 rounded-full border-2 border-accent/20 border-t-accent animate-spin"></div>
      </div>
    {:else if loadError}
      <div class="flex flex-col items-center justify-center h-full gap-3">
        <p class="text-base text-red-500">{loadError}</p>
        <button class="px-3 py-1.5 rounded-md bg-surface-raised border border-border text-text-secondary hover:bg-surface-sunken text-ui transition-colors"
          onclick={() => loadContent(entry.id)}>Retry</button>
      </div>
    {:else if entry.content_type === 'pdf' && content?.pdf_base64}
      <PdfView data={content.pdf_base64} {initialPage} />
    {:else if viewMode === 'archive' && content?.snapshot_html}
      <iframe bind:this={iframeEl} sandbox="allow-same-origin" class="archive-frame" title="Archived page snapshot"></iframe>
    {:else if content?.readable_html}
      <div class="overflow-y-auto h-full bg-surface-raised">
        <article class="max-w-2xl mx-auto px-6 py-8 prose prose-sm prose-gray dark:prose-invert">
          {@html decodeBase64(content.readable_html)}
        </article>
      </div>
    {:else if content?.extracted_text}
      <div class="overflow-y-auto h-full bg-surface-raised">
        <div class="max-w-2xl mx-auto px-6 py-8 text-base text-text-secondary whitespace-pre-wrap leading-relaxed">
          {content.extracted_text}
        </div>
      </div>
    {:else}
      <div class="flex items-center justify-center h-full text-text-tertiary text-base">No content available</div>
    {/if}
  </div>
</div>
