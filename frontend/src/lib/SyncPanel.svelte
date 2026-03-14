<script lang="ts">
  import { api, type SyncStatus, type SyncResultItem, formatDate } from '../api';

  let status = $state<SyncStatus | null>(null);
  let syncResults = $state<SyncResultItem[]>([]);
  let isSyncing = $state(false);
  let isLoading = $state(true);
  let error = $state('');
  let addPeerId = $state('');
  let addPeerName = $state('');
  let showAddForm = $state(false);
  let copied = $state(false);

  async function loadStatus() {
    isLoading = true;
    error = '';
    try {
      status = await api.getSyncStatus();
    } catch (err) {
      error = `Failed to load sync status: ${err}`;
    } finally {
      isLoading = false;
    }
  }

  async function handleSync() {
    isSyncing = true;
    syncResults = [];
    error = '';
    try {
      syncResults = await api.triggerSync();
      await loadStatus();
    } catch (err) {
      error = `Sync failed: ${err}`;
    } finally {
      isSyncing = false;
    }
  }

  async function handleAddPeer() {
    if (!addPeerId.trim()) return;
    error = '';
    try {
      await api.addSyncPeer(addPeerId.trim(), addPeerName.trim() || undefined);
      addPeerId = '';
      addPeerName = '';
      showAddForm = false;
      await loadStatus();
    } catch (err) {
      error = `Failed to add peer: ${err}`;
    }
  }

  async function handleRemovePeer(id: string) {
    error = '';
    try {
      await api.removeSyncPeer(id);
      await loadStatus();
    } catch (err) {
      error = `Failed to remove peer: ${err}`;
    }
  }

  function copyNodeId() {
    if (status?.node_id) {
      navigator.clipboard.writeText(status.node_id);
      copied = true;
      setTimeout(() => { copied = false; }, 2000);
    }
  }

  function truncateId(id: string): string {
    return id.length > 20 ? id.slice(0, 10) + '...' + id.slice(-10) : id;
  }

  loadStatus();
</script>

<div class="flex-1 overflow-y-auto p-6 max-w-2xl">
  <h2 class="text-xl font-semibold text-gray-900 dark:text-gray-100 mb-6">P2P Sync</h2>

  {#if isLoading}
    <p class="text-gray-500 dark:text-gray-400">Loading sync status...</p>
  {:else if status}
    <!-- Node info -->
    <div class="bg-gray-50 dark:bg-gray-800 rounded-lg p-4 mb-6">
      <div class="flex items-center justify-between mb-2">
        <h3 class="text-sm font-medium text-gray-700 dark:text-gray-300">This Node</h3>
        <span class="text-xs px-2 py-0.5 rounded-full {status.sync_available ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400' : 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400'}">
          {status.sync_available ? 'Active' : 'Unavailable'}
        </span>
      </div>

      {#if status.node_id}
        <div class="flex items-center gap-2 mt-2">
          <code class="text-xs bg-white dark:bg-gray-900 px-2 py-1 rounded border border-gray-200 dark:border-gray-700 font-mono flex-1 truncate text-gray-900 dark:text-gray-100">
            {status.node_id}
          </code>
          <button
            class="text-xs px-2 py-1 rounded bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 hover:bg-gray-100 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 transition-colors shrink-0"
            onclick={copyNodeId}
          >
            {copied ? 'Copied!' : 'Copy'}
          </button>
        </div>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">Share this ID with peers to connect</p>
      {:else}
        <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
          cr-sqlite extension not found. Install cr-sqlite to enable P2P sync.
        </p>
      {/if}
    </div>

    <!-- Sync button -->
    {#if status.sync_available}
      <div class="flex items-center gap-3 mb-6">
        <button
          class="px-4 py-2 bg-gray-900 dark:bg-gray-100 text-white dark:text-gray-900 text-sm rounded-md hover:bg-gray-800 dark:hover:bg-gray-200 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          onclick={handleSync}
          disabled={isSyncing || status.peers.length === 0}
        >
          {isSyncing ? 'Syncing...' : 'Sync Now'}
        </button>

        {#if syncResults.length > 0}
          <div class="text-sm">
            {#each syncResults as result}
              <span class={result.success ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}>
                {truncateId(result.peer_id)}:
                {#if result.success}
                  sent {result.sent}, received {result.received}
                {:else}
                  {result.error}
                {/if}
              </span>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Peers -->
    <div class="mb-6">
      <div class="flex items-center justify-between mb-3">
        <h3 class="text-sm font-medium text-gray-700 dark:text-gray-300">
          Peers ({status.peers.length})
        </h3>
        {#if status.sync_available}
          <button
            class="text-xs px-2 py-1 rounded bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 transition-colors"
            onclick={() => { showAddForm = !showAddForm; }}
          >
            {showAddForm ? 'Cancel' : '+ Add Peer'}
          </button>
        {/if}
      </div>

      {#if showAddForm}
        <div class="bg-gray-50 dark:bg-gray-800 rounded-lg p-4 mb-3">
          <div class="space-y-2">
            <input
              type="text"
              class="w-full px-3 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md focus:outline-none focus:ring-1 focus:ring-gray-400 font-mono bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100"
              placeholder="Peer Node ID"
              bind:value={addPeerId}
            />
            <input
              type="text"
              class="w-full px-3 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md focus:outline-none focus:ring-1 focus:ring-gray-400 bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100"
              placeholder="Name (optional)"
              bind:value={addPeerName}
            />
            <button
              class="px-3 py-1.5 text-sm bg-gray-900 dark:bg-gray-100 text-white dark:text-gray-900 rounded-md hover:bg-gray-800 dark:hover:bg-gray-200 transition-colors disabled:opacity-50"
              onclick={handleAddPeer}
              disabled={!addPeerId.trim()}
            >
              Add Peer
            </button>
          </div>
        </div>
      {/if}

      {#if status.peers.length === 0}
        <p class="text-sm text-gray-500 dark:text-gray-400">
          No peers configured. Add a peer's Node ID to start syncing.
        </p>
      {:else}
        <div class="space-y-2">
          {#each status.peers as peer}
            <div class="bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg p-3 flex items-start justify-between">
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="text-sm font-medium text-gray-900 dark:text-gray-100">
                    {peer.name || '(unnamed)'}
                  </span>
                </div>
                <code class="text-xs text-gray-500 dark:text-gray-400 font-mono block truncate mt-0.5">
                  {peer.id}
                </code>
                <div class="text-xs text-gray-400 dark:text-gray-500 mt-1">
                  {#if peer.last_sync_at}
                    Last sync: {formatDate(peer.last_sync_at)}
                  {:else}
                    Never synced
                  {/if}
                  &middot; Version: {peer.last_db_version}
                </div>
              </div>
              <button
                class="text-xs text-red-500 dark:text-red-400 hover:text-red-700 dark:hover:text-red-300 transition-colors ml-2 shrink-0"
                onclick={() => handleRemovePeer(peer.id)}
              >
                Remove
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if error}
    <div class="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-3 text-sm text-red-700 dark:text-red-400">
      {error}
    </div>
  {/if}
</div>
