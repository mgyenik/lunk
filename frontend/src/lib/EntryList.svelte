<script lang="ts">
  import { formatDate, type Entry, type SearchHit } from '../api';

  interface Props {
    entries: (Entry | SearchHit)[];
    totalCount: number;
    isLoading: boolean;
    currentView: string;
    searchQuery: string;
    activeTag: string | null;
    contentTypeFilter: 'all' | 'article' | 'pdf';
    onSelect: (entry: Entry, matchedPage?: number) => void;
    onTagsChange: (id: string, tags: string[]) => void;
    onContentTypeFilter: (type: 'all' | 'article' | 'pdf') => void;
  }
  let { entries, totalCount, isLoading, currentView, searchQuery, activeTag, contentTypeFilter, onSelect, onTagsChange, onContentTypeFilter }: Props = $props();

  let focusedIndex = $state(-1);

  $effect(() => {
    entries;
    focusedIndex = -1;
  });

  function viewTitle(): string {
    if (currentView === 'search') return `Search: "${searchQuery}"`;
    if (activeTag) return activeTag;
    return 'All Entries';
  }

  function hasSnippet(entry: Entry | SearchHit): entry is SearchHit {
    return 'snippet' in entry && entry.snippet !== null;
  }

  function hasFilters(): boolean {
    return activeTag !== null || contentTypeFilter !== 'all';
  }

  function entryInitial(entry: Entry | SearchHit): string {
    return (entry.title?.[0] ?? '?').toUpperCase();
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

  const contentTypeFilters = [
    { id: 'all' as const, label: 'All' },
    { id: 'article' as const, label: 'Articles' },
    { id: 'pdf' as const, label: 'PDFs' },
  ];
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex-1 flex flex-col overflow-hidden">
  <!-- Header -->
  <div class="px-4 py-3 border-b border-gray-100 dark:border-gray-800">
    <div class="flex items-center justify-between gap-3">
      <h2 class="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">{viewTitle()}</h2>

      <div class="flex items-center gap-2 shrink-0">
        <!-- Content type filter pills -->
        <div class="flex items-center gap-1">
          {#each contentTypeFilters as filter}
            <button
              class="px-2.5 py-0.5 rounded-full text-[11px] font-medium transition-colors
                {contentTypeFilter === filter.id
                  ? 'bg-accent text-white'
                  : 'bg-gray-100 dark:bg-gray-800 text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300'}"
              onclick={() => onContentTypeFilter(filter.id)}
            >
              {filter.label}
            </button>
          {/each}
        </div>

        <span class="text-[11px] text-gray-400 dark:text-gray-500 tabular-nums">{totalCount}</span>
      </div>
    </div>
  </div>

  <!-- Entry list -->
  <div class="flex-1 overflow-y-auto">
    {#if isLoading}
      <div class="flex items-center justify-center py-16 text-gray-400 dark:text-gray-500 text-sm">
        Loading...
      </div>
    {:else if entries.length === 0}
      <!-- Empty states -->
      {#if currentView === 'search'}
        <!-- Empty search -->
        <div class="flex flex-col items-center justify-center py-16 px-8">
          <svg class="w-10 h-10 text-gray-200 dark:text-gray-700 mb-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
          </svg>
          <p class="text-sm font-medium text-gray-500 dark:text-gray-400">No results for "{searchQuery}"</p>
          <p class="text-xs text-gray-400 dark:text-gray-500 mt-1">Try different search terms</p>
        </div>
      {:else if hasFilters()}
        <!-- Empty filtered -->
        <div class="flex flex-col items-center justify-center py-16 px-8">
          <svg class="w-10 h-10 text-gray-200 dark:text-gray-700 mb-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path d="M12 3c2.755 0 5.455.232 8.083.678.533.09.917.556.917 1.096v1.044a2.25 2.25 0 01-.659 1.591l-5.432 5.432a2.25 2.25 0 00-.659 1.591v2.927a2.25 2.25 0 01-1.244 2.013L9.75 21v-6.568a2.25 2.25 0 00-.659-1.591L3.659 7.409A2.25 2.25 0 013 5.818V4.774c0-.54.384-1.006.917-1.096A48.32 48.32 0 0112 3z" />
          </svg>
          <p class="text-sm text-gray-500 dark:text-gray-400">No entries match this filter</p>
          <p class="text-xs text-gray-400 dark:text-gray-500 mt-1">Try removing the tag or content type filter</p>
        </div>
      {:else}
        <!-- Empty archive — onboarding -->
        <div class="flex flex-col items-center justify-center py-16 px-8 max-w-sm mx-auto">
          <div class="w-14 h-14 rounded-2xl bg-accent-soft flex items-center justify-center mb-5">
            <svg class="w-7 h-7 text-accent" viewBox="0 0 48 46" fill="currentColor">
              <path d="M25.946 44.938c-.664.845-2.021.375-2.021-.698V33.937a2.26 2.26 0 0 0-2.262-2.262H10.287c-.92 0-1.456-1.04-.92-1.788l7.48-10.471c1.07-1.497 0-3.578-1.842-3.578H1.237c-.92 0-1.456-1.04-.92-1.788L10.013.474c.214-.297.556-.474.92-.474h28.894c.92 0 1.456 1.04.92 1.788l-7.48 10.471c-1.07 1.498 0 3.579 1.842 3.579h11.377c.943 0 1.473 1.088.89 1.83L25.947 44.94z"/>
            </svg>
          </div>
          <h3 class="text-base font-semibold text-gray-900 dark:text-gray-100 mb-1">Get started with Lunk</h3>
          <p class="text-sm text-gray-500 dark:text-gray-400 text-center mb-6">
            Save articles and documents to your personal archive
          </p>
          <div class="w-full space-y-2.5 text-left">
            <div class="flex items-start gap-3 p-3 rounded-lg bg-gray-50 dark:bg-gray-800/50 border border-gray-100 dark:border-gray-700/50">
              <span class="text-accent font-bold text-sm mt-0.5">1</span>
              <div>
                <p class="text-sm font-medium text-gray-700 dark:text-gray-300">Install the browser extension</p>
                <p class="text-xs text-gray-400 dark:text-gray-500 mt-0.5">Save any webpage with one click or Alt+S</p>
              </div>
            </div>
            <div class="flex items-start gap-3 p-3 rounded-lg bg-gray-50 dark:bg-gray-800/50 border border-gray-100 dark:border-gray-700/50">
              <span class="text-accent font-bold text-sm mt-0.5">2</span>
              <div>
                <p class="text-sm font-medium text-gray-700 dark:text-gray-300">Import a PDF</p>
                <p class="text-xs text-gray-400 dark:text-gray-500 mt-0.5">Drag and drop or use the sidebar button</p>
              </div>
            </div>
            <div class="flex items-start gap-3 p-3 rounded-lg bg-gray-50 dark:bg-gray-800/50 border border-gray-100 dark:border-gray-700/50">
              <span class="text-accent font-bold text-sm mt-0.5">3</span>
              <div>
                <p class="text-sm font-medium text-gray-700 dark:text-gray-300">Use the CLI</p>
                <p class="text-xs text-gray-400 dark:text-gray-500 mt-0.5">
                  <code class="text-[11px] bg-gray-100 dark:bg-gray-700 px-1.5 py-0.5 rounded font-mono">lunk save &lt;url&gt;</code>
                </p>
              </div>
            </div>
          </div>
        </div>
      {/if}
    {:else}
      <div class="divide-y divide-gray-100 dark:divide-gray-800/50">
        {#each entries as entry, i (entry.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div
            data-entry-index={i}
            tabindex="-1"
            role="option"
            aria-selected={focusedIndex === i}
            class="w-full text-left px-4 py-3.5 hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors cursor-pointer
              {focusedIndex === i ? 'entry-focused' : ''}"
            onclick={() => onSelect(entry, 'matched_page' in entry ? (entry as SearchHit).matched_page ?? undefined : undefined)}
          >
            <div class="flex items-start gap-3">
              <!-- Favicon / Letter avatar -->
              <div class="w-8 h-8 rounded-lg shrink-0 flex items-center justify-center mt-0.5
                {entry.content_type === 'pdf'
                  ? 'bg-red-50 dark:bg-red-900/20'
                  : 'bg-accent-soft'}">
                {#if entry.content_type === 'article' && entry.domain}
                  <img
                    src="https://www.google.com/s2/favicons?domain={entry.domain}&sz=16"
                    alt=""
                    class="w-4 h-4 rounded-sm"
                    onerror={(e) => { (e.target as HTMLImageElement).style.display = 'none'; (e.target as HTMLImageElement).nextElementSibling?.classList.remove('hidden'); }}
                  />
                  <span class="hidden text-xs font-bold text-accent">{entryInitial(entry)}</span>
                {:else if entry.content_type === 'pdf'}
                  <svg class="w-4 h-4 text-red-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path d="M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z" />
                  </svg>
                {:else}
                  <span class="text-xs font-bold text-accent">{entryInitial(entry)}</span>
                {/if}
              </div>

              <div class="flex-1 min-w-0">
                <!-- Tags row -->
                <div class="flex items-center gap-1.5 mb-0.5 flex-wrap">
                  <span class="text-[10px] px-1.5 py-0.5 rounded font-medium uppercase tracking-wide
                    {entry.content_type === 'pdf'
                      ? 'bg-red-50 dark:bg-red-900/20 text-red-500 dark:text-red-400'
                      : 'bg-accent-soft text-accent'}">
                    {entry.content_type === 'pdf' ? 'PDF' : 'Article'}
                  </span>
                  {#each entry.tags as tag}
                    <span class="text-[10px] px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-700/50 text-gray-400 dark:text-gray-500">
                      {tag}
                    </span>
                  {/each}
                  {#if entry.domain}
                    <span class="text-[10px] text-gray-400 dark:text-gray-500 truncate">{entry.domain}</span>
                  {/if}
                </div>

                <!-- Title -->
                <h3 class="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate leading-snug">{entry.title}</h3>

                <!-- Search snippet -->
                {#if hasSnippet(entry) && entry.snippet}
                  <p class="text-xs text-gray-500 dark:text-gray-400 mt-1 line-clamp-2">{@html entry.snippet}</p>
                {/if}

                <!-- Meta -->
                <div class="flex items-center gap-3 mt-1.5 text-[11px] text-gray-400 dark:text-gray-500">
                  <span>{formatDate(entry.created_at)}</span>
                  {#if entry.word_count}
                    <span>{entry.word_count.toLocaleString()} words</span>
                  {/if}
                  {#if entry.page_count}
                    <span>{entry.page_count} pg</span>
                  {/if}
                  {#if 'matched_page' in entry && entry.matched_page}
                    <span class="text-accent font-medium">Match on p.{entry.matched_page}</span>
                  {/if}
                </div>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
