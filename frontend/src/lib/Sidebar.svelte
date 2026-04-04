<script lang="ts">
  import { api, type TagWithCount } from '../api';

  interface Props {
    currentView: 'all' | 'search' | 'sync';
    activeTag: string | null;
    tagsRefreshKey: number;
    onNavigate: (view: 'all' | 'sync') => void;
    onTagSelect: (tag: string | null) => void;
    onImportPdf: () => void;
  }
  let { currentView, activeTag, tagsRefreshKey, onNavigate, onTagSelect, onImportPdf }: Props = $props();

  let isDark = $state(document.documentElement.classList.contains('dark'));
  let tags = $state<TagWithCount[]>([]);
  let tagsExpanded = $state(true);

  $effect(() => {
    tagsRefreshKey;
    api.getTags().then(t => { tags = t; });
  });

  function toggleDark() {
    isDark = !isDark;
    document.documentElement.classList.toggle('dark', isDark);
    localStorage.theme = isDark ? 'dark' : 'light';
  }
</script>

<aside class="w-[232px] bg-surface-sunken border-r border-border flex flex-col h-full shrink-0 relative texture-noise">
  <!-- Brand -->
  <div class="px-4 pt-5 pb-4">
    <div class="flex items-center gap-2.5">
      <div class="w-7 h-7 rounded-lg bg-accent flex items-center justify-center shadow-sm shadow-accent/20">
        <svg class="w-4 h-4 text-white" viewBox="0 0 48 46" fill="currentColor">
          <path d="M25.946 44.938c-.664.845-2.021.375-2.021-.698V33.937a2.26 2.26 0 0 0-2.262-2.262H10.287c-.92 0-1.456-1.04-.92-1.788l7.48-10.471c1.07-1.497 0-3.578-1.842-3.578H1.237c-.92 0-1.456-1.04-.92-1.788L10.013.474c.214-.297.556-.474.92-.474h28.894c.92 0 1.456 1.04.92 1.788l-7.48 10.471c-1.07 1.498 0 3.579 1.842 3.579h11.377c.943 0 1.473 1.088.89 1.83L25.947 44.94z"/>
        </svg>
      </div>
      <div>
        <h1 class="font-brand text-base font-bold tracking-wider text-text-primary">LUNK</h1>
        <p class="text-[9px] uppercase tracking-[0.15em] text-text-tertiary font-medium -mt-0.5">Archive</p>
      </div>
    </div>
  </div>

  <!-- Nav -->
  <nav class="flex-1 overflow-y-auto px-2 pb-2">
    <button
      class="w-full text-left px-3 py-[7px] rounded-md text-[13px] flex items-center gap-2.5 mb-0.5 transition-all duration-150
        {currentView === 'all' && activeTag === null
          ? 'bg-accent-soft text-accent font-medium shadow-sm shadow-accent/5'
          : 'text-text-secondary hover:bg-surface-raised hover:text-text-primary'}"
      onclick={() => onNavigate('all')}
    >
      <svg class="w-[15px] h-[15px] opacity-50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
        <path d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
      </svg>
      All Entries
    </button>

    <!-- Tags -->
    {#if tags.length > 0}
      <div class="mt-4">
        <button
          class="w-full text-left px-3 py-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-text-tertiary flex items-center gap-1.5 hover:text-text-secondary transition-colors"
          onclick={() => tagsExpanded = !tagsExpanded}
        >
          <svg class="w-2.5 h-2.5 transition-transform duration-150 {tagsExpanded ? 'rotate-90' : ''}" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
            <path d="M9 5l7 7-7 7" />
          </svg>
          Tags
        </button>

        {#if tagsExpanded}
          <div class="mt-1 space-y-px max-h-[45vh] overflow-y-auto">
            {#each tags as tag}
              <button
                class="w-full text-left px-3 py-[6px] rounded-md text-[12px] flex items-center justify-between transition-all duration-150
                  {activeTag === tag.name
                    ? 'bg-accent-soft text-accent font-medium'
                    : 'text-text-secondary hover:bg-surface-raised hover:text-text-primary'}"
                onclick={() => onTagSelect(activeTag === tag.name ? null : tag.name)}
              >
                <span class="truncate">{tag.name}</span>
                <span class="font-brand shrink-0 text-[10px] tabular-nums
                  {activeTag === tag.name ? 'text-accent/60' : 'text-text-tertiary'}"
                >{tag.count}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </nav>

  <!-- Actions -->
  <div class="px-2 pb-2 space-y-1">
    <button
      class="w-full text-[13px] px-3 py-[7px] rounded-md bg-accent text-white hover:bg-accent-hover transition-colors flex items-center gap-2 justify-center font-medium shadow-sm shadow-accent/20"
      onclick={onImportPdf}
    >
      <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
        <path d="M12 4v16m8-8H4" />
      </svg>
      Import PDF
    </button>

    <button
      class="w-full text-left px-3 py-[7px] rounded-md text-[13px] flex items-center gap-2.5 transition-all duration-150
        {currentView === 'sync'
          ? 'bg-accent-soft text-accent font-medium'
          : 'text-text-secondary hover:bg-surface-raised hover:text-text-primary'}"
      onclick={() => onNavigate('sync')}
    >
      <svg class="w-[15px] h-[15px] opacity-50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
        <path d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
      </svg>
      P2P Sync
    </button>
  </div>

  <!-- Footer -->
  <div class="px-3 py-2.5 border-t border-border flex items-center justify-between">
    <span class="font-brand text-[10px] text-text-tertiary">v0.2.0</span>
    <button
      class="text-[10px] px-2 py-0.5 rounded-md text-text-tertiary hover:text-text-secondary hover:bg-surface-raised transition-colors"
      onclick={toggleDark}
      title="Toggle dark mode"
    >
      {isDark ? '&#9728;' : '&#9790;'}
    </button>
  </div>
</aside>
