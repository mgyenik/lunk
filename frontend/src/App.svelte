<script lang="ts">
  import Sidebar from './lib/Sidebar.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import EntryList from './lib/EntryList.svelte';
  import EntryView from './lib/EntryView.svelte';
  import SyncPanel from './lib/SyncPanel.svelte';
  import { api, type Entry, type SearchHit } from './api';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { open } from '@tauri-apps/plugin-dialog';

  let currentView = $state<'all' | 'read-later' | 'search' | 'sync'>('all');
  let entries = $state<(Entry | SearchHit)[]>([]);
  let totalCount = $state(0);
  let selectedEntry = $state<Entry | null>(null);
  let selectedMatchedPage = $state<number | undefined>(undefined);
  let searchQuery = $state('');
  let isLoading = $state(false);
  let isDragOver = $state(false);

  async function loadEntries() {
    isLoading = true;
    try {
      if (currentView === 'search' && searchQuery.trim()) {
        const result = await api.search(searchQuery, 100);
        entries = result.entries;
        totalCount = result.total;
      } else if (currentView === 'read-later') {
        const result = await api.listEntries({ tag: 'read-later', limit: 100 });
        entries = result.entries;
        totalCount = result.total;
      } else {
        const result = await api.listEntries({ limit: 100 });
        entries = result.entries;
        totalCount = result.total;
      }
    } catch (err) {
      console.error('Failed to load entries:', err);
    } finally {
      isLoading = false;
    }
  }

  function handleNavigate(view: 'all' | 'read-later' | 'sync') {
    currentView = view;
    selectedEntry = null;
    selectedMatchedPage = undefined;
    searchQuery = '';
    if (view !== 'sync') {
      loadEntries();
    }
  }

  function handleSearch(query: string) {
    searchQuery = query;
    if (query.trim()) {
      currentView = 'search';
    } else {
      currentView = 'all';
    }
    selectedEntry = null;
    selectedMatchedPage = undefined;
    loadEntries();
  }

  function handleSelect(entry: Entry, matchedPage?: number) {
    selectedEntry = entry;
    selectedMatchedPage = matchedPage;
  }

  async function handleTagsChange(id: string, tags: string[]) {
    try {
      const updated = await api.updateTags(id, tags);
      if (selectedEntry?.id === id) {
        selectedEntry = updated;
      }
      loadEntries();
    } catch (err) {
      console.error('Failed to update tags:', err);
    }
  }

  async function handleDelete(id: string) {
    try {
      await api.deleteEntry(id);
      if (selectedEntry?.id === id) {
        selectedEntry = null;
        selectedMatchedPage = undefined;
      }
      loadEntries();
    } catch (err) {
      console.error('Failed to delete:', err);
    }
  }

  function handleBack() {
    selectedEntry = null;
    selectedMatchedPage = undefined;
  }

  async function handleImportPdf() {
    const path = await open({
      filters: [{ name: 'PDF', extensions: ['pdf'] }],
      multiple: false,
    });
    if (path) {
      try {
        await api.importPdf(path as string);
        loadEntries();
      } catch (err) {
        console.error('Failed to import PDF:', err);
      }
    }
  }

  // Drag-and-drop PDF import
  $effect(() => {
    let unlisten: (() => void) | null = null;
    const appWindow = getCurrentWebviewWindow();

    appWindow.onDragDropEvent((event) => {
      if (event.payload.type === 'enter' || event.payload.type === 'over') {
        isDragOver = true;
      } else if (event.payload.type === 'leave') {
        isDragOver = false;
      } else if (event.payload.type === 'drop') {
        isDragOver = false;
        const paths = event.payload.paths ?? [];
        for (const path of paths) {
          if (path.toLowerCase().endsWith('.pdf')) {
            api.importPdf(path)
              .then(() => loadEntries())
              .catch(err => console.error('Drop import failed:', err));
          }
        }
      }
    }).then(fn => { unlisten = fn; });

    return () => { unlisten?.(); };
  });

  loadEntries();
</script>

<div class="flex h-full bg-white dark:bg-gray-900 relative">
  {#if isDragOver}
    <div class="absolute inset-0 z-50 bg-blue-500/10 dark:bg-blue-500/20 border-2 border-dashed border-blue-400 rounded-lg flex items-center justify-center pointer-events-none">
      <div class="bg-white/90 dark:bg-gray-800/90 px-6 py-4 rounded-lg shadow-lg text-blue-600 dark:text-blue-400 text-lg font-medium">
        Drop PDF to import
      </div>
    </div>
  {/if}

  <Sidebar
    {currentView}
    onNavigate={handleNavigate}
    onImportPdf={handleImportPdf}
  />

  <div class="flex-1 flex flex-col min-w-0">
    {#if currentView === 'sync'}
      <SyncPanel />
    {:else}
      <SearchBar
        value={searchQuery}
        onSearch={handleSearch}
      />

      {#if selectedEntry}
        <EntryView
          entry={selectedEntry}
          initialPage={selectedMatchedPage}
          onBack={handleBack}
          onTagsChange={handleTagsChange}
          onDelete={handleDelete}
        />
      {:else}
        <EntryList
          {entries}
          {totalCount}
          {isLoading}
          {currentView}
          {searchQuery}
          onSelect={handleSelect}
          onTagsChange={handleTagsChange}
        />
      {/if}
    {/if}
  </div>
</div>
