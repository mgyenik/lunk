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
    if (currentView === 'search') return `"${searchQuery}"`;
    if (activeTag) return `#${activeTag}`;
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
      case 'j': case 'ArrowDown':
        e.preventDefault();
        focusedIndex = Math.min(focusedIndex + 1, entries.length - 1);
        scrollToFocused();
        break;
      case 'k': case 'ArrowUp':
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
    document.querySelector(`[data-entry-index="${focusedIndex}"]`)?.scrollIntoView({ block: 'nearest' });
  }

  const typeFilters = [
    { id: 'all' as const, label: 'All' },
    { id: 'article' as const, label: 'Articles' },
    { id: 'pdf' as const, label: 'PDFs' },
  ];
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex-1 flex flex-col overflow-hidden bg-surface">
  <!-- Header -->
  <div class="px-4 py-2.5 border-b border-border-subtle flex items-center justify-between gap-3 shrink-0">
    <div class="flex items-center gap-2 min-w-0">
      <h2 class="text-[13px] font-semibold text-text-primary truncate">{viewTitle()}</h2>
      <span class="font-brand text-[10px] text-text-tertiary tabular-nums shrink-0">{totalCount}</span>
    </div>

    <div class="flex items-center gap-0.5 shrink-0">
      {#each typeFilters as filter}
        <button
          class="px-2.5 py-[3px] rounded-md text-[11px] font-medium transition-all duration-150
            {contentTypeFilter === filter.id
              ? 'bg-accent text-white shadow-sm shadow-accent/20'
              : 'text-text-tertiary hover:text-text-secondary hover:bg-surface-raised'}"
          onclick={() => onContentTypeFilter(filter.id)}
        >
          {filter.label}
        </button>
      {/each}
    </div>
  </div>

  <!-- List -->
  <div class="flex-1 overflow-y-auto">
    {#if isLoading}
      <div class="flex items-center justify-center py-20">
        <div class="w-5 h-5 rounded-full border-2 border-accent/20 border-t-accent animate-spin"></div>
      </div>
    {:else if entries.length === 0}
      {#if currentView === 'search'}
        <div class="flex flex-col items-center justify-center py-20 px-8">
          <div class="w-10 h-10 rounded-xl bg-surface-sunken flex items-center justify-center mb-3">
            <svg class="w-5 h-5 text-text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
            </svg>
          </div>
          <p class="text-[13px] font-medium text-text-secondary">No results for "{searchQuery}"</p>
          <p class="text-[11px] text-text-tertiary mt-1">Try different search terms</p>
        </div>
      {:else if hasFilters()}
        <div class="flex flex-col items-center justify-center py-20 px-8">
          <div class="w-10 h-10 rounded-xl bg-surface-sunken flex items-center justify-center mb-3">
            <svg class="w-5 h-5 text-text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path d="M12 3c2.755 0 5.455.232 8.083.678.533.09.917.556.917 1.096v1.044a2.25 2.25 0 01-.659 1.591l-5.432 5.432a2.25 2.25 0 00-.659 1.591v2.927a2.25 2.25 0 01-1.244 2.013L9.75 21v-6.568a2.25 2.25 0 00-.659-1.591L3.659 7.409A2.25 2.25 0 013 5.818V4.774c0-.54.384-1.006.917-1.096A48.32 48.32 0 0112 3z" />
            </svg>
          </div>
          <p class="text-[13px] text-text-secondary">No entries match this filter</p>
          <p class="text-[11px] text-text-tertiary mt-1">Try removing a filter</p>
        </div>
      {:else}
        <!-- Onboarding -->
        <div class="flex flex-col items-center justify-center py-16 px-8 max-w-xs mx-auto">
          <div class="w-12 h-12 rounded-2xl bg-accent flex items-center justify-center mb-5 shadow-lg shadow-accent/20">
            <svg class="w-6 h-6 text-white" viewBox="0 0 48 46" fill="currentColor">
              <path d="M25.946 44.938c-.664.845-2.021.375-2.021-.698V33.937a2.26 2.26 0 0 0-2.262-2.262H10.287c-.92 0-1.456-1.04-.92-1.788l7.48-10.471c1.07-1.497 0-3.578-1.842-3.578H1.237c-.92 0-1.456-1.04-.92-1.788L10.013.474c.214-.297.556-.474.92-.474h28.894c.92 0 1.456 1.04.92 1.788l-7.48 10.471c-1.07 1.498 0 3.579 1.842 3.579h11.377c.943 0 1.473 1.088.89 1.83L25.947 44.94z"/>
            </svg>
          </div>
          <h3 class="text-[15px] font-semibold text-text-primary mb-1">Start your archive</h3>
          <p class="text-[12px] text-text-secondary text-center mb-6 leading-relaxed">
            Save articles, papers, and documents you want to keep forever.
          </p>
          <div class="w-full space-y-2 text-left">
            {#each [
              { n: '1', title: 'Browser extension', desc: 'Save any page with Alt+S' },
              { n: '2', title: 'Import a PDF', desc: 'Drag & drop or use the sidebar' },
              { n: '3', title: 'CLI', desc: 'lunk save <url>' },
            ] as step}
              <div class="flex items-start gap-3 p-2.5 rounded-lg bg-surface-raised border border-border-subtle">
                <span class="font-brand text-accent text-[11px] font-bold mt-px">{step.n}</span>
                <div>
                  <p class="text-[12px] font-medium text-text-primary">{step.title}</p>
                  <p class="text-[11px] text-text-tertiary">{step.desc}</p>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    {:else}
      {#each entries as entry, i (entry.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div
          data-entry-index={i}
          tabindex="-1"
          role="option"
          aria-selected={focusedIndex === i}
          class="px-4 py-3 border-b border-border-subtle hover:bg-surface-raised transition-all duration-100 cursor-pointer
            {focusedIndex === i ? 'entry-focused' : ''}"
          onclick={() => onSelect(entry, 'matched_page' in entry ? (entry as SearchHit).matched_page ?? undefined : undefined)}
        >
          <div class="flex items-start gap-3">
            <!-- Icon -->
            <div class="w-7 h-7 rounded-md shrink-0 flex items-center justify-center mt-0.5
              {entry.content_type === 'pdf' ? 'bg-red-50 dark:bg-red-950/30' : 'bg-accent-soft'}">
              {#if entry.content_type === 'article' && entry.domain}
                <img
                  src="https://www.google.com/s2/favicons?domain={entry.domain}&sz=16"
                  alt=""
                  class="w-3.5 h-3.5 rounded-sm"
                  onerror={(e) => { (e.target as HTMLImageElement).style.display = 'none'; (e.target as HTMLImageElement).nextElementSibling?.classList.remove('hidden'); }}
                />
                <span class="hidden font-brand text-[10px] font-bold text-accent">{entryInitial(entry)}</span>
              {:else if entry.content_type === 'pdf'}
                <svg class="w-3.5 h-3.5 text-red-400 dark:text-red-500/70" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path d="M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z" />
                </svg>
              {:else}
                <span class="font-brand text-[10px] font-bold text-accent">{entryInitial(entry)}</span>
              {/if}
            </div>

            <div class="flex-1 min-w-0">
              <!-- Meta line -->
              <div class="flex items-center gap-1.5 mb-[3px]">
                <span class="font-brand text-[9px] font-semibold uppercase tracking-wider
                  {entry.content_type === 'pdf' ? 'text-red-400 dark:text-red-500/70' : 'text-accent/70'}">
                  {entry.content_type === 'pdf' ? 'PDF' : 'WEB'}
                </span>
                {#if entry.domain}
                  <span class="text-[10px] text-text-tertiary truncate">{entry.domain}</span>
                {/if}
              </div>

              <!-- Title -->
              <h3 class="text-[13px] font-semibold text-text-primary truncate leading-snug">{entry.title}</h3>

              <!-- Tags -->
              {#if entry.tags.length > 0}
                <div class="flex items-center gap-1 mt-1 flex-wrap">
                  {#each entry.tags as tag}
                    <span class="text-[10px] px-1.5 py-[1px] rounded bg-surface-sunken text-text-tertiary">#{tag}</span>
                  {/each}
                </div>
              {/if}

              <!-- Snippet -->
              {#if hasSnippet(entry) && entry.snippet}
                <p class="text-[11px] text-text-secondary mt-1 line-clamp-2 leading-relaxed">{@html entry.snippet}</p>
              {/if}

              <!-- Footer -->
              <div class="flex items-center gap-2.5 mt-1.5 font-brand text-[10px] text-text-tertiary">
                <span>{formatDate(entry.created_at)}</span>
                {#if entry.word_count}
                  <span>{entry.word_count.toLocaleString()}w</span>
                {/if}
                {#if entry.page_count}
                  <span>{entry.page_count}pg</span>
                {/if}
                {#if 'matched_page' in entry && entry.matched_page}
                  <span class="text-accent font-semibold">p.{entry.matched_page}</span>
                {/if}
              </div>
            </div>
          </div>
        </div>
      {/each}
    {/if}
  </div>
</div>
