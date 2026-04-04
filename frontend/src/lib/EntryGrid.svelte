<script lang="ts">
  import { formatDate, type Entry } from '../api';

  interface Props {
    entries: Entry[];
    totalCount: number;
    isLoading: boolean;
    activeTopic: string | null;
    onSelect: (entry: Entry) => void;
    onBack: () => void;
  }
  let { entries, totalCount, isLoading, activeTopic, onSelect, onBack }: Props = $props();

  let focusedIndex = $state(-1);

  $effect(() => {
    entries;
    focusedIndex = -1;
  });

  function entryInitial(entry: Entry): string {
    return (entry.title?.[0] ?? '?').toUpperCase();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (entries.length === 0) return;

    const cols = Math.max(1, Math.floor((document.querySelector('.card-grid')?.clientWidth ?? 600) / 195));

    switch (e.key) {
      case 'j': case 'ArrowDown':
        e.preventDefault();
        focusedIndex = Math.min(focusedIndex + cols, entries.length - 1);
        scrollToFocused();
        break;
      case 'k': case 'ArrowUp':
        e.preventDefault();
        focusedIndex = Math.max(focusedIndex - cols, 0);
        scrollToFocused();
        break;
      case 'ArrowRight': case 'l':
        e.preventDefault();
        focusedIndex = Math.min(focusedIndex + 1, entries.length - 1);
        scrollToFocused();
        break;
      case 'ArrowLeft': case 'h':
        e.preventDefault();
        focusedIndex = Math.max(focusedIndex - 1, 0);
        scrollToFocused();
        break;
      case 'Enter':
        if (focusedIndex >= 0) onSelect(entries[focusedIndex]);
        break;
      case 'Escape':
        onBack();
        break;
    }
  }

  function scrollToFocused() {
    document.querySelector(`[data-card-index="${focusedIndex}"]`)?.scrollIntoView({ block: 'nearest' });
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex-1 flex flex-col overflow-hidden bg-surface">
  <!-- Header -->
  <div class="px-5 py-3 border-b border-border-subtle flex items-center gap-3 shrink-0">
    <button
      class="text-text-secondary hover:text-accent transition-colors flex items-center gap-1"
      onclick={onBack}
      aria-label="Go back"
    >
      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path d="M15 19l-7-7 7-7" />
      </svg>
    </button>
    <h2 class="text-[14px] font-semibold text-text-primary">
      {activeTopic ?? 'All Entries'}
    </h2>
    <span class="font-brand text-[11px] text-text-tertiary">{totalCount}</span>
  </div>

  <!-- Grid -->
  <div class="flex-1 overflow-y-auto p-4">
    {#if isLoading}
      <div class="flex items-center justify-center py-20">
        <div class="w-5 h-5 rounded-full border-2 border-accent/20 border-t-accent animate-spin"></div>
      </div>
    {:else if entries.length === 0}
      <div class="flex flex-col items-center justify-center py-20">
        <p class="text-[13px] text-text-secondary">No entries</p>
      </div>
    {:else}
      <div class="card-grid">
        {#each entries as entry, i (entry.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div
            data-card-index={i}
            role="option"
            aria-selected={focusedIndex === i}
            class="p-3 rounded-lg bg-surface-raised border transition-all cursor-pointer group
              {focusedIndex === i
                ? 'border-accent shadow-sm shadow-accent/10'
                : 'border-border-subtle hover:border-accent/30 hover:shadow-sm'}"
            onclick={() => onSelect(entry)}
          >
            <!-- Icon row -->
            <div class="flex items-center gap-1.5 mb-2">
              {#if entry.content_type === 'article' && entry.domain}
                <img
                  src="https://www.google.com/s2/favicons?domain={entry.domain}&sz=16"
                  alt="" class="w-3.5 h-3.5 rounded-sm"
                  onerror={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
                />
              {:else}
                <div class="w-3.5 h-3.5 rounded-sm flex items-center justify-center
                  {entry.content_type === 'pdf' ? 'bg-red-50 dark:bg-red-950/30' : 'bg-accent-soft'}">
                  <span class="text-[7px] font-bold
                    {entry.content_type === 'pdf' ? 'text-red-400' : 'text-accent'}">{entryInitial(entry)}</span>
                </div>
              {/if}
              <span class="font-brand text-[9px] text-text-tertiary truncate flex-1">{entry.domain ?? ''}</span>
              <span class="font-brand text-[8px] text-text-tertiary uppercase">{entry.content_type === 'pdf' ? 'PDF' : 'WEB'}</span>
            </div>

            <!-- Title -->
            <h3 class="text-[13px] font-semibold text-text-primary line-clamp-2 leading-snug mb-2 group-hover:text-accent transition-colors">
              {entry.title}
            </h3>

            <!-- Tags -->
            {#if entry.tags.length > 0}
              <div class="flex flex-wrap gap-1 mb-2">
                {#each entry.tags.slice(0, 3) as tag}
                  <span class="text-[9px] px-1.5 py-[1px] rounded bg-surface-sunken text-text-tertiary">#{tag}</span>
                {/each}
              </div>
            {/if}

            <!-- Footer -->
            <div class="flex items-center gap-2 font-brand text-[10px] text-text-tertiary">
              <span>{formatDate(entry.created_at)}</span>
              {#if entry.word_count}
                <span>{entry.word_count.toLocaleString()}w</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
