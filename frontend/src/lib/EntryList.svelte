<script lang="ts">
  import { formatDate, type Entry, type SearchHit } from '../api';

  interface Props {
    entries: (Entry | SearchHit)[];
    totalCount: number;
    isLoading: boolean;
    currentView: string;
    searchQuery: string;
    onSelect: (entry: Entry, matchedPage?: number) => void;
    onTagsChange: (id: string, tags: string[]) => void;
  }
  let { entries, totalCount, isLoading, currentView, searchQuery, onSelect, onTagsChange }: Props = $props();

  let focusedIndex = $state(-1);

  // Reset focus when entries change
  $effect(() => {
    entries;
    focusedIndex = -1;
  });

  function viewTitle(): string {
    if (currentView === 'search') return `Search: "${searchQuery}"`;
    if (currentView === 'read-later') return 'Read Later';
    return 'All Entries';
  }

  function hasSnippet(entry: Entry | SearchHit): entry is SearchHit {
    return 'snippet' in entry && entry.snippet !== null;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (entries.length === 0) return;

    switch (e.key) {
      case 'j':
      case 'ArrowDown':
        e.preventDefault();
        focusedIndex = Math.min(focusedIndex + 1, entries.length - 1);
        scrollToFocused();
        break;
      case 'k':
      case 'ArrowUp':
        e.preventDefault();
        focusedIndex = Math.max(focusedIndex - 1, 0);
        scrollToFocused();
        break;
      case 'Enter':
        if (focusedIndex >= 0 && focusedIndex < entries.length) {
          const entry = entries[focusedIndex];
          const matchedPage = 'matched_page' in entry ? (entry as SearchHit).matched_page ?? undefined : undefined;
          onSelect(entry, matchedPage);
        }
        break;
    }
  }

  function scrollToFocused() {
    const el = document.querySelector(`[data-entry-index="${focusedIndex}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  }

  function toggleReadLater(e: MouseEvent, entry: Entry | SearchHit) {
    e.stopPropagation();
    const hasTag = entry.tags.includes('read-later');
    const newTags = hasTag
      ? entry.tags.filter(t => t !== 'read-later')
      : [...entry.tags, 'read-later'];
    onTagsChange(entry.id, newTags);
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex-1 overflow-y-auto">
  <div class="px-4 py-3 border-b border-gray-100 dark:border-gray-800">
    <div class="flex items-center justify-between">
      <h2 class="text-sm font-medium text-gray-900 dark:text-gray-100">{viewTitle()}</h2>
      <span class="text-xs text-gray-500 dark:text-gray-400">{totalCount} entries</span>
    </div>
  </div>

  {#if isLoading}
    <div class="flex items-center justify-center py-12 text-gray-400 dark:text-gray-500 text-sm">
      Loading...
    </div>
  {:else if entries.length === 0}
    <div class="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-gray-500">
      <p class="text-sm">
        {#if currentView === 'search'}
          No results for "{searchQuery}"
        {:else if currentView === 'read-later'}
          No read-later entries
        {:else}
          No entries yet
        {/if}
      </p>
      <p class="text-xs mt-1">Save pages with the browser extension or CLI</p>
    </div>
  {:else}
    <div class="divide-y divide-gray-100 dark:divide-gray-800">
      {#each entries as entry, i (entry.id)}
        <button
          data-entry-index={i}
          class="w-full text-left px-4 py-3 hover:bg-gray-50 dark:hover:bg-gray-800 transition-colors group
            {focusedIndex === i ? 'bg-blue-50 dark:bg-blue-900/20 entry-focused' : ''}"
          onclick={() => onSelect(entry, 'matched_page' in entry ? (entry as SearchHit).matched_page ?? undefined : undefined)}
        >
          <div class="flex items-start gap-3">
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 mb-0.5">
                <span class="text-xs px-1.5 py-0.5 rounded font-medium
                  {entry.content_type === 'pdf' ? 'bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400' : 'bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'}">
                  {entry.content_type === 'pdf' ? 'PDF' : 'Article'}
                </span>
                {#each entry.tags as tag}
                  <span class="text-xs px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400">
                    {tag}
                  </span>
                {/each}
                {#if entry.domain}
                  <span class="text-xs text-gray-400 dark:text-gray-500 truncate">{entry.domain}</span>
                {/if}
              </div>

              <h3 class="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">{entry.title}</h3>

              {#if hasSnippet(entry) && entry.snippet}
                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1 line-clamp-2">{@html entry.snippet}</p>
              {/if}

              <div class="flex items-center gap-3 mt-1 text-xs text-gray-400 dark:text-gray-500">
                <span>{formatDate(entry.created_at)}</span>
                {#if entry.word_count}
                  <span>{entry.word_count.toLocaleString()} words</span>
                {/if}
                {#if entry.page_count}
                  <span>{entry.page_count} pages</span>
                {/if}
                {#if 'matched_page' in entry && entry.matched_page}
                  <span class="text-yellow-600 dark:text-yellow-400">Match on p.{entry.matched_page}</span>
                {/if}
              </div>
            </div>

            <!-- Quick action: toggle read-later -->
            <div class="opacity-0 group-hover:opacity-100 flex gap-1 shrink-0 transition-opacity">
              <button
                class="text-xs px-2 py-1 rounded transition-colors
                  {entry.tags.includes('read-later')
                    ? 'bg-yellow-50 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400 hover:bg-yellow-100 dark:hover:bg-yellow-900/50'
                    : 'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600'}"
                onclick={(e: MouseEvent) => toggleReadLater(e, entry)}
              >
                {entry.tags.includes('read-later') ? 'Remove read-later' : 'Read later'}
              </button>
            </div>
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>
