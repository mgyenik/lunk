<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import NavRail from './lib/NavRail.svelte';
  import HomeView from './lib/HomeView.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import EntryList from './lib/EntryList.svelte';
  import EntryGrid from './lib/EntryGrid.svelte';
  import EntryView from './lib/EntryView.svelte';
  import SyncPanel from './lib/SyncPanel.svelte';
  import { api, type Entry, type SearchHit, type TopicSummary, type ArchiveStats } from './api';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { open } from '@tauri-apps/plugin-dialog';

  type ViewType = 'home' | 'browse' | 'search' | 'detail' | 'sync';

  let currentView = $state<ViewType>('home');
  let entries = $state<(Entry | SearchHit)[]>([]);
  let totalCount = $state(0);
  let selectedEntry = $state<Entry | null>(null);
  let selectedMatchedPage = $state<number | undefined>(undefined);
  let searchQuery = $state('');
  let isLoading = $state(false);
  let isDragOver = $state(false);

  // Home data
  let topics = $state<TopicSummary[]>([]);
  let stats = $state<ArchiveStats | null>(null);
  let recentEntries = $state<Entry[]>([]);

  // Browse state
  let activeTopic = $state<string | null>(null);

  async function loadHomeData() {
    try {
      const [t, s, r] = await Promise.all([
        api.getTopics(),
        api.getArchiveStats(),
        api.listEntries({ limit: 8 }),
      ]);
      topics = t;
      stats = s;
      recentEntries = r.entries;
    } catch (err) {
      console.error('Failed to load home data:', err);
    }
  }

  async function loadBrowseEntries() {
    isLoading = true;
    try {
      if (activeTopic) {
        const result = await api.getTopicEntries(activeTopic);
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

  async function loadSearchResults() {
    if (!searchQuery.trim()) return;
    isLoading = true;
    try {
      const result = await api.search(searchQuery, 100);
      entries = result.entries;
      totalCount = result.total;
    } catch (err) {
      console.error('Failed to search:', err);
    } finally {
      isLoading = false;
    }
  }

  function handleNavigate(view: 'home' | 'sync') {
    currentView = view;
    selectedEntry = null;
    selectedMatchedPage = undefined;
    searchQuery = '';
    activeTopic = null;
    if (view === 'home') loadHomeData();
  }

  function handleSearch(query: string) {
    searchQuery = query;
    if (query.trim()) {
      currentView = 'search';
      loadSearchResults();
    } else {
      currentView = 'home';
      loadHomeData();
    }
    selectedEntry = null;
    selectedMatchedPage = undefined;
  }

  function handleTopicSelect(label: string) {
    activeTopic = label;
    currentView = 'browse';
    loadBrowseEntries();
  }

  function handleBrowseAll() {
    activeTopic = null;
    currentView = 'browse';
    loadBrowseEntries();
  }

  function handleSelect(entry: Entry, matchedPage?: number) {
    selectedEntry = entry;
    selectedMatchedPage = matchedPage;
    currentView = 'detail';
  }

  function handleBack() {
    selectedEntry = null;
    selectedMatchedPage = undefined;
    // Go back to wherever we came from
    if (searchQuery.trim()) {
      currentView = 'search';
    } else if (activeTopic !== null) {
      currentView = 'browse';
    } else {
      currentView = 'home';
    }
  }

  async function handleTagsChange(id: string, tags: string[]) {
    try {
      const updated = await api.updateTags(id, tags);
      if (selectedEntry?.id === id) selectedEntry = updated;
      loadHomeData();
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
        currentView = 'home';
      }
      loadHomeData();
    } catch (err) {
      console.error('Failed to delete:', err);
    }
  }

  async function handleImportPdf() {
    const path = await open({
      filters: [{ name: 'PDF', extensions: ['pdf'] }],
      multiple: false,
    });
    if (path) {
      try {
        await api.importPdf(path as string);
        loadHomeData();
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
              .then(() => loadHomeData())
              .catch(err => console.error('Drop import failed:', err));
          }
        }
      }
    }).then(fn => { unlisten = fn; });

    return () => { unlisten?.(); };
  });

  // Load home data on startup
  loadHomeData();
</script>

<div class="flex h-full bg-surface relative">
  {#if isDragOver}
    <div class="absolute inset-0 z-50 bg-accent/8 border-2 border-dashed border-accent/40 rounded-lg flex items-center justify-center pointer-events-none">
      <div class="bg-surface-raised px-6 py-4 rounded-xl shadow-xl shadow-accent/10 border border-accent/20">
        <p class="text-accent text-[15px] font-semibold">Drop PDF to import</p>
      </div>
    </div>
  {/if}

  <NavRail {currentView} onNavigate={handleNavigate} onImportPdf={handleImportPdf} />

  <div class="flex-1 flex flex-col min-w-0">
    {#if currentView === 'home'}
      <HomeView
        {topics} {stats} {recentEntries}
        onSearch={handleSearch}
        onTopicSelect={handleTopicSelect}
        onEntrySelect={handleSelect}
        onBrowseAll={handleBrowseAll}
      />
    {:else if currentView === 'detail' && selectedEntry}
      <div class="flex-1 min-h-0 flex flex-col" in:fly={{ x: 20, duration: 150 }} out:fade={{ duration: 80 }}>
        <EntryView
          entry={selectedEntry}
          initialPage={selectedMatchedPage}
          onBack={handleBack}
          onTagsChange={handleTagsChange}
          onDelete={handleDelete}
          onNavigate={(e) => handleSelect(e)}
        />
      </div>
    {:else if currentView === 'search'}
      <SearchBar value={searchQuery} onSearch={handleSearch} />
      <EntryList
        {entries} {totalCount} {isLoading}
        currentView="search"
        {searchQuery}
        onSelect={handleSelect}
      />
    {:else if currentView === 'browse'}
      <EntryGrid
        {entries} {totalCount} {isLoading}
        {activeTopic}
        onSelect={handleSelect}
        onBack={() => handleNavigate('home')}
      />
    {:else if currentView === 'sync'}
      <SyncPanel />
    {/if}
  </div>
</div>
