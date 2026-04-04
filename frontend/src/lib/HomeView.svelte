<script lang="ts">
  import { formatDate, type Entry, type TopicSummary, type ArchiveStats } from '../api';

  interface Props {
    topics: TopicSummary[];
    stats: ArchiveStats | null;
    recentEntries: Entry[];
    onSearch: (query: string) => void;
    onTopicSelect: (label: string) => void;
    onEntrySelect: (entry: Entry) => void;
    onBrowseAll: () => void;
  }
  let { topics, stats, recentEntries, onSearch, onTopicSelect, onEntrySelect, onBrowseAll }: Props = $props();

  let searchValue = $state('');
  let searchEl = $state<HTMLInputElement | undefined>(undefined);
  let searchFocused = $state(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  function handleGlobalKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (e.key === '/' && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      searchEl?.focus();
    }
  }

  function handleSearchInput() {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      if (searchValue.trim()) onSearch(searchValue);
    }, 300);
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      if (debounceTimer) clearTimeout(debounceTimer);
      if (searchValue.trim()) onSearch(searchValue);
    }
    if (e.key === 'Escape') {
      searchValue = '';
      searchEl?.blur();
    }
  }

  function entryInitial(entry: Entry): string {
    return (entry.title?.[0] ?? '?').toUpperCase();
  }
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="flex-1 overflow-y-auto bg-surface">
  {#if recentEntries.length === 0 && !stats?.total_entries}
    <!-- Onboarding (empty archive) -->
    <div class="flex flex-col items-center justify-center h-full px-8 max-w-sm mx-auto">
      <div class="w-14 h-14 rounded-2xl bg-accent flex items-center justify-center mb-6 shadow-lg shadow-accent/20">
        <svg class="w-7 h-7 text-white" viewBox="0 0 48 46" fill="currentColor">
          <path d="M25.946 44.938c-.664.845-2.021.375-2.021-.698V33.937a2.26 2.26 0 0 0-2.262-2.262H10.287c-.92 0-1.456-1.04-.92-1.788l7.48-10.471c1.07-1.497 0-3.578-1.842-3.578H1.237c-.92 0-1.456-1.04-.92-1.788L10.013.474c.214-.297.556-.474.92-.474h28.894c.92 0 1.456 1.04.92 1.788l-7.48 10.471c-1.07 1.498 0 3.579 1.842 3.579h11.377c.943 0 1.473 1.088.89 1.83L25.947 44.94z"/>
        </svg>
      </div>
      <h2 class="text-[17px] font-semibold text-text-primary mb-1">Start your archive</h2>
      <p class="text-[13px] text-text-secondary text-center mb-6 leading-relaxed">
        Save articles, papers, and documents you want to keep forever.
      </p>
      <div class="w-full space-y-2 text-left">
        {#each [
          { n: '1', title: 'Browser extension', desc: 'Save any page with Alt+S' },
          { n: '2', title: 'Import a PDF', desc: 'Drag & drop or use the + button' },
          { n: '3', title: 'CLI', desc: 'lunk save <url>' },
        ] as step}
          <div class="flex items-start gap-3 p-3 rounded-lg bg-surface-raised border border-border-subtle">
            <span class="font-brand text-accent text-[12px] font-bold mt-px">{step.n}</span>
            <div>
              <p class="text-[12px] font-medium text-text-primary">{step.title}</p>
              <p class="text-[11px] text-text-tertiary">{step.desc}</p>
            </div>
          </div>
        {/each}
      </div>
    </div>
  {:else}
    <!-- Dashboard -->
    <div class="max-w-2xl mx-auto px-6 py-8">
      <!-- Hero search -->
      <div class="mb-8">
        <div class="flex items-center gap-3 px-4 py-3 rounded-xl bg-surface-raised border transition-all duration-200
          {searchFocused ? 'border-accent/30 shadow-lg shadow-accent/5' : 'border-border shadow-sm'}">
          <svg class="w-5 h-5 shrink-0 transition-colors {searchFocused ? 'text-accent' : 'text-text-tertiary'}"
            fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" />
          </svg>
          <input
            bind:this={searchEl}
            type="text"
            placeholder="Search your archive..."
            class="flex-1 text-[16px] outline-none bg-transparent text-text-primary placeholder-text-tertiary"
            bind:value={searchValue}
            oninput={handleSearchInput}
            onkeydown={handleSearchKeydown}
            onfocus={() => searchFocused = true}
            onblur={() => searchFocused = false}
          />
          {#if !searchValue && !searchFocused}
            <kbd class="font-brand text-[11px] px-2 py-0.5 rounded-md border border-border text-text-tertiary">/</kbd>
          {/if}
        </div>
      </div>

      <!-- Stats -->
      {#if stats}
        <div class="flex items-center gap-4 mb-8 font-brand text-[11px] text-text-tertiary">
          <span>{stats.total_entries} entries</span>
          <span class="w-1 h-1 rounded-full bg-text-tertiary/40"></span>
          <span>{stats.pdf_count} PDFs</span>
          <span class="w-1 h-1 rounded-full bg-text-tertiary/40"></span>
          <span>{stats.article_count} articles</span>
          <span class="w-1 h-1 rounded-full bg-text-tertiary/40"></span>
          <span>{stats.domain_count} domains</span>
          {#if stats.recent_count > 0}
            <span class="w-1 h-1 rounded-full bg-text-tertiary/40"></span>
            <span class="text-accent">{stats.recent_count} this week</span>
          {/if}
        </div>
      {/if}

      <!-- Recent -->
      {#if recentEntries.length > 0}
        <div class="mb-8">
          <div class="flex items-center justify-between mb-3">
            <h3 class="text-[11px] font-semibold uppercase tracking-[0.1em] text-text-tertiary">Recent</h3>
            <button
              class="text-[11px] text-text-tertiary hover:text-accent transition-colors"
              onclick={onBrowseAll}
            >Browse all &rarr;</button>
          </div>
          <div class="flex gap-3 overflow-x-auto pb-2 scroll-x-hidden">
            {#each recentEntries as entry (entry.id)}
              <button
                class="shrink-0 w-[160px] p-3 rounded-lg bg-surface-raised border border-border-subtle text-left
                  hover:border-accent/30 hover:shadow-sm transition-all cursor-pointer group"
                onclick={() => onEntrySelect(entry)}
              >
                <div class="flex items-center gap-1.5 mb-2">
                  {#if entry.content_type === 'article' && entry.domain}
                    <img
                      src="https://www.google.com/s2/favicons?domain={entry.domain}&sz=16"
                      alt="" class="w-3.5 h-3.5 rounded-sm"
                      onerror={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                    />
                  {:else}
                    <div class="w-3.5 h-3.5 rounded-sm {entry.content_type === 'pdf' ? 'bg-red-100 dark:bg-red-900/30' : 'bg-accent-soft'} flex items-center justify-center">
                      <span class="text-[7px] font-bold {entry.content_type === 'pdf' ? 'text-red-400' : 'text-accent'}">{entryInitial(entry)}</span>
                    </div>
                  {/if}
                  <span class="font-brand text-[9px] text-text-tertiary truncate">{entry.domain ?? entry.content_type.toUpperCase()}</span>
                </div>
                <h4 class="text-[12px] font-semibold text-text-primary line-clamp-2 leading-snug mb-1.5 group-hover:text-accent transition-colors">
                  {entry.title}
                </h4>
                <span class="font-brand text-[9px] text-text-tertiary">{formatDate(entry.created_at)}</span>
              </button>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Topics -->
      {#if topics.length > 0}
        <div>
          <h3 class="text-[11px] font-semibold uppercase tracking-[0.1em] text-text-tertiary mb-3">Topics</h3>
          <div class="flex flex-wrap gap-2">
            {#each topics as topic}
              <button
                class="inline-flex items-center gap-2 px-3 py-2 rounded-lg bg-surface-raised border border-border-subtle
                  hover:border-accent/30 hover:bg-accent-soft/50 cursor-pointer transition-all text-left group"
                onclick={() => onTopicSelect(topic.label)}
                title={topic.sample_titles.join('\n')}
              >
                <span class="text-[12px] font-medium text-text-primary group-hover:text-accent transition-colors">{topic.label}</span>
                <span class="font-brand text-[10px] text-text-tertiary">{topic.entry_count}</span>
              </button>
            {/each}
          </div>
        </div>
      {:else if stats && stats.total_entries > 0 && stats.total_entries < 10}
        <div class="text-center py-4">
          <p class="text-[12px] text-text-tertiary">Topics will appear as your archive grows</p>
        </div>
      {/if}
    </div>
  {/if}
</div>
