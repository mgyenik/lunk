<script lang="ts">
  interface Props {
    currentView: 'all' | 'queue' | 'archived' | 'search' | 'sync';
    onNavigate: (view: 'all' | 'queue' | 'archived' | 'sync') => void;
    onImportPdf: () => void;
  }
  let { currentView, onNavigate, onImportPdf }: Props = $props();

  let isDark = $state(document.documentElement.classList.contains('dark'));

  const navItems = [
    { id: 'queue' as const, label: 'Read Queue', icon: '&#9776;' },
    { id: 'all' as const, label: 'All Entries', icon: '&#9733;' },
    { id: 'archived' as const, label: 'Archived', icon: '&#9745;' },
  ];

  function toggleDark() {
    isDark = !isDark;
    document.documentElement.classList.toggle('dark', isDark);
    localStorage.theme = isDark ? 'dark' : 'light';
  }
</script>

<aside class="w-56 bg-gray-50 dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 flex flex-col h-full shrink-0">
  <div class="p-4 border-b border-gray-200 dark:border-gray-700">
    <h1 class="text-lg font-bold tracking-wider text-gray-900 dark:text-gray-100">LUNK</h1>
    <p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">Personal Archive</p>
  </div>

  <nav class="flex-1 p-2">
    {#each navItems as item}
      <button
        class="w-full text-left px-3 py-2 rounded-md text-sm flex items-center gap-2 mb-0.5 transition-colors
          {currentView === item.id
            ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 font-medium'
            : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 hover:text-gray-900 dark:hover:text-gray-200'}"
        onclick={() => onNavigate(item.id)}
      >
        <span class="text-base opacity-60">{@html item.icon}</span>
        {item.label}
      </button>
    {/each}
  </nav>

  <!-- Import PDF -->
  <div class="p-3 border-t border-gray-200 dark:border-gray-700">
    <button
      class="w-full text-sm px-3 py-2 rounded-md bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors flex items-center gap-2 justify-center"
      onclick={onImportPdf}
    >
      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path d="M12 4v16m8-8H4" />
      </svg>
      Import PDF
    </button>
  </div>

  <!-- Sync -->
  <div class="p-2 border-t border-gray-200 dark:border-gray-700">
    <button
      class="w-full text-left px-3 py-2 rounded-md text-sm flex items-center gap-2 transition-colors
        {currentView === 'sync'
          ? 'bg-gray-200 dark:bg-gray-700 text-gray-900 dark:text-gray-100 font-medium'
          : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 hover:text-gray-900 dark:hover:text-gray-200'}"
      onclick={() => onNavigate('sync')}
    >
      <svg class="w-4 h-4 opacity-60" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
      </svg>
      P2P Sync
    </button>
  </div>

  <!-- Footer: dark mode toggle + version -->
  <div class="p-3 border-t border-gray-200 dark:border-gray-700 flex items-center justify-between">
    <span class="text-xs text-gray-400 dark:text-gray-500">v0.1.0</span>
    <button
      class="text-xs px-2 py-1 rounded bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors"
      onclick={toggleDark}
      title="Toggle dark mode"
    >
      {isDark ? 'Light' : 'Dark'}
    </button>
  </div>
</aside>
