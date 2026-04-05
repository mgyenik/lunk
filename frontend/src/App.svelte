<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import NavRail from './lib/NavRail.svelte';
  import HomeView from './lib/HomeView.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import FilterBar from './lib/FilterBar.svelte';
  import EntryList from './lib/EntryList.svelte';
  import EntryGrid from './lib/EntryGrid.svelte';
  import EntryView from './lib/EntryView.svelte';
  import SyncPanel from './lib/SyncPanel.svelte';
  import SettingsPanel from './lib/SettingsPanel.svelte';
  import WelcomeFlow from './lib/WelcomeFlow.svelte';
  import { api, type Entry, type SearchHit, type TopicSummary, type ArchiveStats, type TagWithCount } from './api';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { open } from '@tauri-apps/plugin-dialog';

  type ViewType = 'home' | 'browse' | 'search' | 'detail' | 'sync' | 'settings';

  let currentView = $state<ViewType>('home');
  let entries = $state<(Entry | SearchHit)[]>([]);
  let totalCount = $state(0);
  let selectedEntry = $state<Entry | null>(null);
  let selectedMatchedPage = $state<number | undefined>(undefined);
  let searchQuery = $state('');
  let isLoading = $state(false);
  let isDragOver = $state(false);
  let showWelcome = $state(false);

  // Home data
  let topics = $state<TopicSummary[]>([]);
  let stats = $state<ArchiveStats | null>(null);
  let recentEntries = $state<Entry[]>([]);
  let allTags = $state<TagWithCount[]>([]);

  // Composable filter state — tag, domain, and content type can combine
  let filterTag = $state<string | null>(null);
  let filterDomain = $state<string | null>(null);
  let filterContentType = $state<'all' | 'article' | 'pdf'>('all');

  // Browse state
  let activeTopic = $state<string | null>(null);

  const hasFilters = $derived(filterTag !== null || filterDomain !== null || filterContentType !== 'all');
  const browseTitle = $derived(() => {
    if (activeTopic) return activeTopic;
    const parts: string[] = [];
    if (filterTag) parts.push(`#${filterTag}`);
    if (filterDomain) parts.push(filterDomain);
    if (filterContentType !== 'all') parts.push(filterContentType === 'pdf' ? 'PDFs' : 'Articles');
    return parts.length > 0 ? parts.join(' + ') : 'All Entries';
  });

  async function loadHomeData() {
    try {
      const [t, s, r, tags] = await Promise.all([
        api.getTopics(),
        api.getArchiveStats(),
        api.listEntries({ limit: 8 }),
        api.getTags(),
      ]);
      topics = t;
      stats = s;
      recentEntries = r.entries;
      allTags = tags;
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
        const result = await api.listEntries({
          tag: filterTag ?? undefined,
          domain: filterDomain ?? undefined,
          contentType: filterContentType === 'all' ? undefined : filterContentType,
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

  // --- Navigation handlers ---

  function handleNavigate(view: 'home' | 'sync' | 'settings') {
    currentView = view;
    selectedEntry = null;
    selectedMatchedPage = undefined;
    searchQuery = '';
    activeTopic = null;
    clearAllFilters();
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
  }

  function handleTopicSelect(label: string) {
    activeTopic = label;
    clearAllFilters();
    currentView = 'browse';
    loadBrowseEntries();
  }

  function handleBrowseAll() {
    activeTopic = null;
    clearAllFilters();
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
    if (searchQuery.trim()) {
      currentView = 'search';
    } else if (activeTopic !== null || hasFilters) {
      currentView = 'browse';
    } else {
      currentView = 'home';
    }
  }

  // --- Filter handlers (the core new functionality) ---

  function handleFilterTag(tag: string) {
    filterTag = tag;
    activeTopic = null;
    selectedEntry = null;
    currentView = 'browse';
    loadBrowseEntries();
  }

  function handleFilterDomain(domain: string) {
    filterDomain = domain;
    activeTopic = null;
    selectedEntry = null;
    currentView = 'browse';
    loadBrowseEntries();
  }

  function handleFilterContentType(type: 'all' | 'article' | 'pdf') {
    filterContentType = type;
    activeTopic = null;
    selectedEntry = null;
    currentView = 'browse';
    loadBrowseEntries();
  }

  function handleSearchKeyword(keyword: string) {
    searchQuery = keyword;
    currentView = 'search';
    selectedEntry = null;
    loadSearchResults();
  }

  function clearAllFilters() {
    filterTag = null;
    filterDomain = null;
    filterContentType = 'all';
  }

  // --- Entry mutation handlers ---

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

  loadHomeData();

  // Check if we should show the welcome flow (no LLM model configured)
  api.getLlmStatus().then(status => {
    if (!status.active_model && !localStorage.getItem('lunk-welcome-dismissed')) {
      showWelcome = true;
    }
  }).catch(() => {}); // Ignore errors — welcome is optional
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
        {topics} {stats} {recentEntries} {allTags}
        onSearch={handleSearch}
        onTopicSelect={handleTopicSelect}
        onEntrySelect={handleSelect}
        onBrowseAll={handleBrowseAll}
        onTagSelect={handleFilterTag}
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
          onTagClick={handleFilterTag}
          onDomainClick={handleFilterDomain}
          onKeywordClick={handleSearchKeyword}
        />
      </div>
    {:else if currentView === 'search'}
      <SearchBar value={searchQuery} onSearch={handleSearch} />
      <EntryList
        {entries} {totalCount} {isLoading}
        currentView="search"
        {searchQuery}
        onSelect={handleSelect}
        onTagClick={handleFilterTag}
        onDomainClick={handleFilterDomain}
      />
    {:else if currentView === 'browse'}
      <FilterBar
        tag={filterTag}
        domain={filterDomain}
        contentType={filterContentType}
        onClearTag={() => { filterTag = null; loadBrowseEntries(); }}
        onClearDomain={() => { filterDomain = null; loadBrowseEntries(); }}
        onClearContentType={() => { filterContentType = 'all'; loadBrowseEntries(); }}
        onClearAll={() => { clearAllFilters(); loadBrowseEntries(); }}
      />
      <EntryGrid
        {entries} {totalCount} {isLoading}
        title={browseTitle()}
        onSelect={handleSelect}
        onBack={() => handleNavigate('home')}
        onTagClick={handleFilterTag}
        onDomainClick={handleFilterDomain}
        onContentTypeClick={handleFilterContentType}
      />
    {:else if currentView === 'sync'}
      <SyncPanel />
    {:else if currentView === 'settings'}
      <SettingsPanel />
    {/if}
  </div>
</div>

{#if showWelcome}
  <WelcomeFlow onDismiss={() => { showWelcome = false; }} />
{/if}
