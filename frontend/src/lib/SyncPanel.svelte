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
  <h2 class="text-[16px] font-semibold text-text-primary mb-1">P2P Sync</h2>
  <p class="text-[12px] text-text-tertiary mb-6">Sync your archive across devices</p>

  {#if isLoading}
    <div class="flex items-center justify-center py-20">
      <div class="w-5 h-5 rounded-full border-2 border-accent/20 border-t-accent animate-spin"></div>
    </div>
  {:else if status}
    <!-- Node info -->
    <div class="rounded-lg bg-surface-raised border border-border p-4 mb-6">
      <div class="flex items-center justify-between mb-2">
        <h3 class="text-[13px] font-semibold text-text-primary">This Node</h3>
        <span class="text-[10px] px-2 py-0.5 rounded-full font-brand {status.sync_available
          ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
          : 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400'}">
          {status.sync_available ? 'Active' : 'Unavailable'}
        </span>
      </div>

      {#if status.node_id}
        <div class="flex items-center gap-2 mt-2">
          <code class="text-[11px] bg-surface-sunken px-2 py-1 rounded border border-border-subtle font-brand flex-1 truncate text-text-primary">
            {status.node_id}
          </code>
          <button
            class="text-[11px] px-2.5 py-1 rounded-md border border-border text-text-secondary hover:bg-surface-raised transition-colors shrink-0"
            onclick={copyNodeId}
          >
            {copied ? 'Copied!' : 'Copy'}
          </button>
        </div>
        <p class="text-[10px] text-text-tertiary mt-1.5">Share this ID with peers to connect</p>
      {:else}
        <p class="text-[12px] text-text-secondary mt-1">
          Sync node is starting...
        </p>
      {/if}
    </div>

    <!-- Sync button -->
    {#if status.sync_available}
      <div class="flex items-center gap-3 mb-6">
        <button
          class="px-4 py-2 bg-accent text-white text-[12px] rounded-md hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          onclick={handleSync}
          disabled={isSyncing || status.peers.length === 0}
        >
          {isSyncing ? 'Syncing...' : 'Sync Now'}
        </button>

        {#if syncResults.length > 0}
          <div class="text-[11px] font-brand">
            {#each syncResults as result}
              <span class={result.success ? 'text-green-600 dark:text-green-400' : 'text-red-500 dark:text-red-400'}>
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
        <h3 class="text-[13px] font-semibold text-text-primary">
          Peers ({status.peers.length})
        </h3>
        {#if status.sync_available}
          <button
            class="text-[11px] px-2.5 py-1 rounded-md border border-border text-text-secondary hover:bg-surface-raised transition-colors"
            onclick={() => { showAddForm = !showAddForm; }}
          >
            {showAddForm ? 'Cancel' : '+ Add Peer'}
          </button>
        {/if}
      </div>

      {#if showAddForm}
        <div class="rounded-lg bg-surface-sunken p-4 mb-3 space-y-2">
          <input
            type="text"
            class="w-full px-3 py-1.5 text-[12px] border border-border rounded-md focus:outline-none focus:ring-1 focus:ring-accent/40 font-brand bg-surface-raised text-text-primary"
            placeholder="Peer Node ID"
            bind:value={addPeerId}
          />
          <input
            type="text"
            class="w-full px-3 py-1.5 text-[12px] border border-border rounded-md focus:outline-none focus:ring-1 focus:ring-accent/40 bg-surface-raised text-text-primary"
            placeholder="Name (optional)"
            bind:value={addPeerName}
          />
          <button
            class="px-3 py-1.5 text-[12px] bg-accent text-white rounded-md hover:bg-accent-hover transition-colors disabled:opacity-50"
            onclick={handleAddPeer}
            disabled={!addPeerId.trim()}
          >
            Add Peer
          </button>
        </div>
      {/if}

      {#if status.peers.length === 0}
        <p class="text-[12px] text-text-secondary">
          No peers configured. Add a peer's Node ID to start syncing.
        </p>
      {:else}
        <div class="space-y-2">
          {#each status.peers as peer}
            <div class="bg-surface-raised border border-border-subtle rounded-lg p-3 flex items-start justify-between">
              <div class="min-w-0 flex-1">
                <span class="text-[12px] font-medium text-text-primary">
                  {peer.name || '(unnamed)'}
                </span>
                <code class="text-[10px] text-text-tertiary font-brand block truncate mt-0.5">
                  {peer.id}
                </code>
                <div class="text-[10px] text-text-tertiary mt-1 font-brand">
                  {#if peer.last_sync_at}
                    Last sync: {formatDate(peer.last_sync_at)}
                  {:else}
                    Never synced
                  {/if}
                  &middot; Version: {peer.last_db_version}
                </div>
              </div>
              <button
                class="text-[11px] text-red-500 dark:text-red-400 hover:text-red-700 dark:hover:text-red-300 transition-colors ml-2 shrink-0"
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
    <div class="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-3 text-[12px] text-red-700 dark:text-red-400">
      {error}
    </div>
  {/if}
</div>
