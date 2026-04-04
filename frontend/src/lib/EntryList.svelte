<script lang="ts">
  import FilterChip from './FilterChip.svelte';
  import { formatDate, type Entry, type SearchHit } from '../api';

  interface Props {
    entries: (Entry | SearchHit)[];
    totalCount: number;
    isLoading: boolean;
    currentView: string;
    searchQuery: string;
    onSelect: (entry: Entry, matchedPage?: number) => void;
    onTagClick: (tag: string) => void;
    onDomainClick: (domain: string) => void;
  }
  let { entries, totalCount, isLoading, currentView, searchQuery, onSelect, onTagClick, onDomainClick }: Props = $props();

  let focusedIndex = $state(-1);

  $effect(() => { entries; focusedIndex = -1; });

  function hasSnippet(entry: Entry | SearchHit): entry is SearchHit {
    return 'snippet' in entry && entry.snippet !== null;
  }

  function entryInitial(entry: Entry | SearchHit): string {
    return (entry.title?.[0] ?? '?').toUpperCase();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (entries.length === 0) return;
    switch (e.key) {
      case 'j': case 'ArrowDown':
        e.preventDefault(); focusedIndex = Math.min(focusedIndex + 1, entries.length - 1); scrollToFocused(); break;
      case 'k': case 'ArrowUp':
        e.preventDefault(); focusedIndex = Math.max(focusedIndex - 1, 0); scrollToFocused(); break;
      case 'Enter':
        if (focusedIndex >= 0 && focusedIndex < entries.length) {
          const entry = entries[focusedIndex];
          const mp = 'matched_page' in entry ? (entry as SearchHit).matched_page ?? undefined : undefined;
          onSelect(entry, mp);
        }
        break;
    }
  }

  function scrollToFocused() {
    document.querySelector(`[data-entry-index="${focusedIndex}"]`)?.scrollIntoView({ block: 'nearest' });
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex-1 flex flex-col overflow-hidden bg-surface">
  <!-- Header -->
  <div class="px-4 py-2.5 border-b border-border-subtle flex items-center gap-2 shrink-0">
    <h2 class="text-[13px] font-semibold text-text-primary truncate">
      {#if currentView === 'search'}Results for "{searchQuery}"{:else}Entries{/if}
    </h2>
    <span class="font-brand text-[10px] text-text-tertiary tabular-nums">{totalCount}</span>
  </div>

  <!-- List -->
  <div class="flex-1 overflow-y-auto">
    {#if isLoading}
      <div class="flex items-center justify-center py-20">
        <div class="w-5 h-5 rounded-full border-2 border-accent/20 border-t-accent animate-spin"></div>
      </div>
    {:else if entries.length === 0}
      <div class="flex flex-col items-center justify-center py-20 px-8">
        <div class="w-10 h-10 rounded-xl bg-surface-sunken flex items-center justify-center mb-3">
          <svg class="w-5 h-5 text-text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
          </svg>
        </div>
        <p class="text-[13px] font-medium text-text-secondary">No results for "{searchQuery}"</p>
        <p class="text-[11px] text-text-tertiary mt-1">Try different search terms</p>
      </div>
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
            <div class="w-7 h-7 rounded-md shrink-0 flex items-center justify-center mt-0.5
              {entry.content_type === 'pdf' ? 'bg-red-50 dark:bg-red-950/30' : 'bg-accent-soft'}">
              {#if entry.content_type === 'article' && entry.domain}
                <img src="https://www.google.com/s2/favicons?domain={entry.domain}&sz=16" alt="" class="w-3.5 h-3.5 rounded-sm"
                  onerror={(e) => { (e.target as HTMLImageElement).style.display = 'none'; (e.target as HTMLImageElement).nextElementSibling?.classList.remove('hidden'); }} />
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
              <!-- Meta row: type + domain (clickable) -->
              <div class="flex items-center gap-1.5 mb-[3px]">
                <span class="font-brand text-[9px] font-semibold uppercase tracking-wider
                  {entry.content_type === 'pdf' ? 'text-red-400 dark:text-red-500/70' : 'text-accent/70'}">
                  {entry.content_type === 'pdf' ? 'PDF' : 'WEB'}
                </span>
                {#if entry.domain}
                  <FilterChip label={entry.domain} variant="domain" onclick={() => onDomainClick(entry.domain!)} />
                {/if}
              </div>

              <!-- Title -->
              <h3 class="text-[13px] font-semibold text-text-primary truncate leading-snug">{entry.title}</h3>

              <!-- Tags — clickable (NEW: search results now show tags) -->
              {#if entry.tags.length > 0}
                <div class="flex items-center gap-1 mt-1 flex-wrap">
                  {#each entry.tags.slice(0, 4) as tag}
                    <FilterChip label={tag} variant="tag" onclick={() => onTagClick(tag)} />
                  {/each}
                </div>
              {/if}

              <!-- Search snippet -->
              {#if hasSnippet(entry) && entry.snippet}
                <p class="text-[11px] text-text-secondary mt-1 line-clamp-2 leading-relaxed">{@html entry.snippet}</p>
              {/if}

              <!-- Footer -->
              <div class="flex items-center gap-2.5 mt-1.5 font-brand text-[10px] text-text-tertiary">
                <span>{formatDate(entry.created_at)}</span>
                {#if entry.word_count}<span>{entry.word_count.toLocaleString()}w</span>{/if}
                {#if entry.page_count}<span>{entry.page_count}pg</span>{/if}
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
