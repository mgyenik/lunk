<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import Sidebar from './lib/Sidebar.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import EntryList from './lib/EntryList.svelte';
  import EntryView from './lib/EntryView.svelte';
  import SyncPanel from './lib/SyncPanel.svelte';
  import { api, type Entry, type SearchHit } from './api';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { open } from '@tauri-apps/plugin-dialog';

  let currentView = $state<'all' | 'search' | 'sync'>('all');
  let entries = $state<(Entry | SearchHit)[]>([]);
  let totalCount = $state(0);
  let selectedEntry = $state<Entry | null>(null);
  let selectedMatchedPage = $state<number | undefined>(undefined);
  let searchQuery = $state('');
  let isLoading = $state(false);
  let isDragOver = $state(false);

  // Filtering
  let activeTag = $state<string | null>(null);
  let contentTypeFilter = $state<'all' | 'article' | 'pdf'>('all');
  let tagsRefreshKey = $state(0);

  async function loadEntries() {
    isLoading = true;
    try {
      if (currentView === 'search' && searchQuery.trim()) {
        const result = await api.search(searchQuery, 100);
        entries = result.entries;
        totalCount = result.total;
      } else {
        const result = await api.listEntries({
          tag: activeTag ?? undefined,
          contentType: contentTypeFilter === 'all' ? undefined : contentTypeFilter,
          limit: 100,
        });
        entries = result.entries;
        totalCount = result.total;
      }
    } catch (err) {
      console.error('Failed to load entries:', err);
    } finally {
      isLoading = false;
    }
  }

  // Reload when filters change
  $effect(() => {
    // Track dependencies
    activeTag;
    contentTypeFilter;
    if (currentView !== 'sync' && currentView !== 'search') {
      loadEntries();
    }
  });

  function handleNavigate(view: 'all' | 'sync') {
    currentView = view;
    selectedEntry = null;
    selectedMatchedPage = undefined;
    searchQuery = '';
    if (view === 'all') {
      activeTag = null;
      contentTypeFilter = 'all';
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

  function handleTagSelect(tag: string | null) {
    activeTag = tag;
    currentView = 'all';
    selectedEntry = null;
    selectedMatchedPage = undefined;
    searchQuery = '';
  }

  function handleContentTypeFilter(type: 'all' | 'article' | 'pdf') {
    contentTypeFilter = type;
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
      tagsRefreshKey++;
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
      tagsRefreshKey++;
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
        tagsRefreshKey++;
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
              .then(() => { tagsRefreshKey++; loadEntries(); })
              .catch(err => console.error('Drop import failed:', err));
          }
        }
      }
    }).then(fn => { unlisten = fn; });

    return () => { unlisten?.(); };
  });
</script>

<div class="flex h-full bg-white dark:bg-gray-900 relative">
  {#if isDragOver}
    <div class="absolute inset-0 z-50 bg-accent/10 border-2 border-dashed border-accent rounded-lg flex items-center justify-center pointer-events-none">
      <div class="bg-white/90 dark:bg-gray-800/90 px-6 py-4 rounded-lg shadow-lg text-accent text-lg font-medium">
        Drop PDF to import
      </div>
    </div>
  {/if}

  <Sidebar
    {currentView}
    {activeTag}
    {tagsRefreshKey}
    onNavigate={handleNavigate}
    onTagSelect={handleTagSelect}
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
        <div class="flex-1 min-h-0" in:fly={{ x: 20, duration: 150 }} out:fade={{ duration: 80 }}>
          <EntryView
            entry={selectedEntry}
            initialPage={selectedMatchedPage}
            onBack={handleBack}
            onTagsChange={handleTagsChange}
            onDelete={handleDelete}
          />
        </div>
      {:else}
        <div class="flex-1 min-h-0" in:fade={{ duration: 100 }}>
          <EntryList
            {entries}
            {totalCount}
            {isLoading}
            {currentView}
            {searchQuery}
            {activeTag}
            {contentTypeFilter}
            onSelect={handleSelect}
            onTagsChange={handleTagsChange}
            onContentTypeFilter={handleContentTypeFilter}
          />
        </div>
      {/if}
    {/if}
  </div>
</div>
