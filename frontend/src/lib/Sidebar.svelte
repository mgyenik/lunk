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

  // Fetch tags on mount and when refresh key changes
  $effect(() => {
    tagsRefreshKey; // track dependency
    api.getTags().then(t => { tags = t; });
  });

  function toggleDark() {
    isDark = !isDark;
    document.documentElement.classList.toggle('dark', isDark);
    localStorage.theme = isDark ? 'dark' : 'light';
  }
</script>

<aside class="w-56 bg-gray-50 dark:bg-gray-800/50 border-r border-gray-200 dark:border-gray-700/50 flex flex-col h-full shrink-0">
  <!-- Header -->
  <div class="px-4 py-4 border-b border-gray-200 dark:border-gray-700/50">
    <div class="flex items-center gap-2">
      <svg class="w-5 h-5 text-accent shrink-0" viewBox="0 0 48 46" fill="currentColor">
        <path d="M25.946 44.938c-.664.845-2.021.375-2.021-.698V33.937a2.26 2.26 0 0 0-2.262-2.262H10.287c-.92 0-1.456-1.04-.92-1.788l7.48-10.471c1.07-1.497 0-3.578-1.842-3.578H1.237c-.92 0-1.456-1.04-.92-1.788L10.013.474c.214-.297.556-.474.92-.474h28.894c.92 0 1.456 1.04.92 1.788l-7.48 10.471c-1.07 1.498 0 3.579 1.842 3.579h11.377c.943 0 1.473 1.088.89 1.83L25.947 44.94z"/>
      </svg>
      <div>
        <h1 class="text-xl font-bold tracking-wider text-gray-900 dark:text-gray-100">LUNK</h1>
        <p class="text-[10px] text-gray-400 dark:text-gray-500 -mt-0.5 tracking-wide">Personal Archive</p>
      </div>
    </div>
  </div>

  <!-- Navigation -->
  <nav class="flex-1 overflow-y-auto p-2">
    <!-- All Entries -->
    <button
      class="w-full text-left px-3 py-2 rounded-md text-sm flex items-center gap-2 mb-1 transition-colors
        {currentView === 'all' && activeTag === null
          ? 'bg-accent-soft text-accent font-medium'
          : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700/50 hover:text-gray-900 dark:hover:text-gray-200'}"
      onclick={() => onNavigate('all')}
    >
      <svg class="w-4 h-4 opacity-60" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
      </svg>
      All Entries
    </button>

    <!-- Tags section -->
    {#if tags.length > 0}
      <div class="mt-3">
        <button
          class="w-full text-left px-3 py-1.5 text-[11px] font-medium uppercase tracking-wider text-gray-400 dark:text-gray-500 flex items-center gap-1.5 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
          onclick={() => tagsExpanded = !tagsExpanded}
        >
          <svg class="w-3 h-3 transition-transform {tagsExpanded ? 'rotate-90' : ''}" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path d="M9 5l7 7-7 7" />
          </svg>
          Tags
          <span class="ml-auto text-[10px] font-normal opacity-60">{tags.length}</span>
        </button>

        {#if tagsExpanded}
          <div class="mt-0.5 space-y-0.5 max-h-[50vh] overflow-y-auto">
            {#each tags as tag}
              <button
                class="w-full text-left px-3 py-1.5 rounded-md text-xs flex items-center justify-between transition-colors
                  {activeTag === tag.name
                    ? 'bg-accent-soft text-accent font-medium'
                    : 'text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700/50 hover:text-gray-700 dark:hover:text-gray-300'}"
                onclick={() => onTagSelect(activeTag === tag.name ? null : tag.name)}
              >
                <span class="truncate">{tag.name}</span>
                <span class="shrink-0 text-[10px] tabular-nums px-1.5 rounded-full
                  {activeTag === tag.name
                    ? 'bg-accent/10 text-accent'
                    : 'bg-gray-200/60 dark:bg-gray-600/40 text-gray-400 dark:text-gray-500'}"
                >{tag.count}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </nav>

  <!-- Import PDF -->
  <div class="p-3 border-t border-gray-200 dark:border-gray-700/50">
    <button
      class="w-full text-sm px-3 py-2 rounded-md border border-accent/20 text-accent hover:bg-accent-soft transition-colors flex items-center gap-2 justify-center font-medium"
      onclick={onImportPdf}
    >
      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path d="M12 4v16m8-8H4" />
      </svg>
      Import PDF
    </button>
  </div>

  <!-- Sync -->
  <div class="px-2 pb-1 border-t border-gray-200 dark:border-gray-700/50 pt-1">
    <button
      class="w-full text-left px-3 py-2 rounded-md text-sm flex items-center gap-2 transition-colors
        {currentView === 'sync'
          ? 'bg-accent-soft text-accent font-medium'
          : 'text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700/50 hover:text-gray-700 dark:hover:text-gray-300'}"
      onclick={() => onNavigate('sync')}
    >
      <svg class="w-4 h-4 opacity-60" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
      </svg>
      P2P Sync
    </button>
  </div>

  <!-- Footer -->
  <div class="p-3 border-t border-gray-200 dark:border-gray-700/50 flex items-center justify-between">
    <span class="text-[10px] text-gray-400 dark:text-gray-500">v0.2.0</span>
    <button
      class="text-[10px] px-2 py-1 rounded-md bg-gray-100 dark:bg-gray-700/50 text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors"
      onclick={toggleDark}
      title="Toggle dark mode"
    >
      {isDark ? '☀ Light' : '☾ Dark'}
    </button>
  </div>
</aside>
