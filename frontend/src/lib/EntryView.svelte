<script lang="ts">
  import { api, decodeBase64, formatDate, type Entry, type EntryContent } from '../api';
  import PdfView from './PdfView.svelte';

  interface Props {
    entry: Entry;
    initialPage?: number;
    onBack: () => void;
    onTagsChange: (id: string, tags: string[]) => void;
    onDelete: (id: string) => void;
  }
  let { entry, initialPage, onBack, onTagsChange, onDelete }: Props = $props();

  let content = $state<EntryContent | null>(null);
  let viewMode = $state<'archive' | 'reader'>('archive');
  let isLoading = $state(true);
  let loadError = $state('');
  let confirmingDelete = $state(false);
  let iframeEl: HTMLIFrameElement | undefined = $state();
  let tagInput = $state('');

  $effect(() => {
    loadContent(entry.id);
  });

  async function loadContent(id: string) {
    isLoading = true;
    loadError = '';
    try {
      content = await api.getEntryContent(id);
      if (!content.snapshot_html) {
        viewMode = 'reader';
      } else {
        viewMode = 'archive';
      }
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
    if (e.key === 'Enter') {
      e.preventDefault();
      addTag();
    }
  }

  function toggleReadLater() {
    const has = entry.tags.includes('read-later');
    const newTags = has
      ? entry.tags.filter(t => t !== 'read-later')
      : [...entry.tags, 'read-later'];
    onTagsChange(entry.id, newTags);
  }

  // Keyboard: Escape to go back
  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (e.key === 'Escape') {
      onBack();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex-1 flex flex-col min-h-0">
  <!-- Toolbar -->
  <div class="flex items-center gap-2 px-4 py-2 border-b border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 shrink-0">
    <button
      class="text-sm text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 flex items-center gap-1"
      onclick={onBack}
    >
      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path d="M15 19l-7-7 7-7" />
      </svg>
      Back
    </button>

    <div class="flex-1"></div>

    <!-- View mode toggle (articles only) -->
    {#if entry.content_type === 'article' && content?.snapshot_html}
      <div class="flex rounded-md border border-gray-200 dark:border-gray-600 text-xs">
        <button
          class="px-2.5 py-1 rounded-l-md transition-colors
            {viewMode === 'archive' ? 'bg-gray-900 dark:bg-gray-100 text-white dark:text-gray-900' : 'text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800'}"
          onclick={() => viewMode = 'archive'}
        >
          Archive
        </button>
        <button
          class="px-2.5 py-1 rounded-r-md transition-colors
            {viewMode === 'reader' ? 'bg-gray-900 dark:bg-gray-100 text-white dark:text-gray-900' : 'text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800'}"
          onclick={() => viewMode = 'reader'}
        >
          Reader
        </button>
      </div>
    {/if}

    <!-- Read later toggle -->
    <button
      class="text-xs px-2.5 py-1 rounded-md border transition-colors
        {entry.tags.includes('read-later')
          ? 'border-yellow-300 dark:border-yellow-700 bg-yellow-50 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400'
          : 'border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800'}"
      onclick={toggleReadLater}
    >
      {entry.tags.includes('read-later') ? 'Read later' : 'Read later'}
    </button>

    <!-- Open original -->
    {#if entry.url}
      <a
        href={entry.url}
        target="_blank"
        rel="noopener noreferrer"
        class="text-xs px-2.5 py-1 rounded-md border border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800"
      >
        Original
      </a>
    {/if}

    <!-- Delete with confirmation -->
    <button
      class="text-xs px-2.5 py-1 rounded-md border transition-colors
        {confirmingDelete
          ? 'border-red-400 bg-red-500 text-white hover:bg-red-600'
          : 'border-red-200 dark:border-red-800 text-red-500 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30'}"
      onclick={handleDelete}
    >
      {confirmingDelete ? 'Confirm Delete?' : 'Delete'}
    </button>
  </div>

  <!-- Entry header -->
  <div class="px-6 py-4 border-b border-gray-100 dark:border-gray-800 bg-gray-50 dark:bg-gray-800/50 shrink-0">
    <h1 class="text-lg font-semibold text-gray-900 dark:text-gray-100 leading-tight">{entry.title}</h1>
    <div class="flex items-center gap-3 mt-2 text-xs text-gray-500 dark:text-gray-400">
      {#if entry.domain}
        <span>{entry.domain}</span>
      {/if}
      <span>{formatDate(entry.created_at)}</span>
      {#if entry.word_count}
        <span>{entry.word_count.toLocaleString()} words</span>
      {/if}
      {#if entry.page_count}
        <span>{entry.page_count} pages</span>
      {/if}
    </div>

    <!-- Tags -->
    <div class="flex items-center gap-1.5 mt-2 flex-wrap">
      {#each entry.tags as tag}
        <span class="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400">
          {tag}
          <button
            class="hover:text-blue-800 dark:hover:text-blue-200"
            onclick={() => removeTag(tag)}
          >&times;</button>
        </span>
      {/each}
      <input
        type="text"
        bind:value={tagInput}
        onkeydown={handleTagKeydown}
        placeholder="+ tag"
        class="text-xs px-2 py-0.5 w-16 bg-transparent border-b border-transparent focus:border-gray-300 dark:focus:border-gray-600 outline-none text-gray-500 dark:text-gray-400 placeholder-gray-400 dark:placeholder-gray-600"
      />
    </div>
  </div>

  <!-- Content area -->
  <div class="flex-1 min-h-0 overflow-hidden">
    {#if isLoading}
      <div class="flex items-center justify-center h-full text-gray-400 dark:text-gray-500 text-sm">
        Loading content...
      </div>
    {:else if loadError}
      <div class="flex flex-col items-center justify-center h-full gap-3 text-sm">
        <p class="text-red-500 dark:text-red-400">{loadError}</p>
        <button
          class="px-3 py-1.5 rounded-md bg-gray-100 dark:bg-gray-800 text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-700 text-xs"
          onclick={() => loadContent(entry.id)}
        >
          Retry
        </button>
      </div>
    {:else if entry.content_type === 'pdf' && content?.pdf_base64}
      <PdfView data={content.pdf_base64} {initialPage} />
    {:else if viewMode === 'archive' && content?.snapshot_html}
      <iframe
        bind:this={iframeEl}
        sandbox="allow-same-origin"
        class="archive-frame"
        title="Archived page snapshot"
      ></iframe>
    {:else if content?.readable_html}
      <div class="overflow-y-auto h-full bg-white dark:bg-gray-900">
        <article class="max-w-2xl mx-auto px-6 py-8 prose prose-sm prose-gray dark:prose-invert">
          {@html decodeBase64(content.readable_html)}
        </article>
      </div>
    {:else if content?.extracted_text}
      <div class="overflow-y-auto h-full bg-white dark:bg-gray-900">
        <div class="max-w-2xl mx-auto px-6 py-8 text-sm text-gray-700 dark:text-gray-300 whitespace-pre-wrap leading-relaxed">
          {content.extracted_text}
        </div>
      </div>
    {:else}
      <div class="flex items-center justify-center h-full text-gray-400 dark:text-gray-500 text-sm">
        No content available
      </div>
    {/if}
  </div>
</div>
