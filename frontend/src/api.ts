import { invoke } from '@tauri-apps/api/core';

export interface Entry {
  id: string;
  url: string | null;
  title: string;
  content_type: 'article' | 'pdf';
  domain: string | null;
  word_count: number | null;
  page_count: number | null;
  index_status: 'ok' | 'partial' | 'failed' | 'pending';
  index_version: number;
  created_at: string;
  updated_at: string;
  saved_by: string;
  tags: string[];
}

export interface SearchHit {
  id: string;
  url: string | null;
  title: string;
  content_type: 'article' | 'pdf';
  domain: string | null;
  word_count: number | null;
  page_count: number | null;
  index_status: 'ok' | 'partial' | 'failed' | 'pending';
  index_version: number;
  created_at: string;
  updated_at: string;
  saved_by: string;
  tags: string[];
  snippet: string | null;
  matched_page: number | null;
}

export interface SearchResult {
  entries: SearchHit[];
  total: number;
}

export interface ListResult {
  entries: Entry[];
  total: number;
}

export interface EntryContent {
  entry_id: string;
  extracted_text: string;
  snapshot_html: string | null;   // base64
  readable_html: string | null;   // base64
  pdf_base64: string | null;
}

export interface TagWithCount {
  name: string;
  count: number;
}

export interface SyncPeer {
  id: string;
  name: string | null;
  last_sync_at: string | null;
  last_db_version: number;
}

export interface SyncStatus {
  sync_available: boolean;
  node_id: string | null;
  peers: SyncPeer[];
}

export interface SyncResultItem {
  peer_id: string;
  success: boolean;
  sent: number | null;
  received: number | null;
  error: string | null;
}

export interface TagSuggestions {
  domain_tags: string[];
  similar_tags: string[];
  popular_tags: string[];
}

export const api = {
  search(query: string, limit = 50, offset = 0): Promise<SearchResult> {
    return invoke('search_entries', { query, limit, offset });
  },

  listEntries(params: {
    contentType?: string;
    tag?: string;
    domain?: string;
    limit?: number;
    offset?: number;
  } = {}): Promise<ListResult> {
    return invoke('list_entries', { params });
  },

  getEntry(id: string): Promise<Entry> {
    return invoke('get_entry', { id });
  },

  getEntryContent(id: string): Promise<EntryContent> {
    return invoke('get_entry_content', { id });
  },

  updateTags(id: string, tags: string[]): Promise<Entry> {
    return invoke('update_entry_tags', { id, tags });
  },

  getTagSuggestions(domain?: string, title?: string): Promise<TagSuggestions> {
    return invoke('get_tag_suggestions', { domain: domain || null, title: title || null });
  },

  deleteEntry(id: string): Promise<void> {
    return invoke('delete_entry', { id });
  },

  getTags(): Promise<TagWithCount[]> {
    return invoke('get_tags');
  },

  importPdf(path: string): Promise<Entry> {
    return invoke('import_pdf', { path });
  },

  getSyncStatus(): Promise<SyncStatus> {
    return invoke('get_sync_status');
  },

  getSyncPeers(): Promise<SyncPeer[]> {
    return invoke('get_sync_peers');
  },

  addSyncPeer(id: string, name?: string): Promise<void> {
    return invoke('add_sync_peer', { id, name: name || null });
  },

  removeSyncPeer(id: string): Promise<void> {
    return invoke('remove_sync_peer', { id });
  },

  triggerSync(): Promise<SyncResultItem[]> {
    return invoke('trigger_sync');
  },
};

export function decodeBase64(b64: string): string {
  const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

export function formatDate(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const diff = now.getTime() - d.getTime();
  const days = Math.floor(diff / (1000 * 60 * 60 * 24));

  if (days === 0) return 'Today';
  if (days === 1) return 'Yesterday';
  if (days < 7) return `${days}d ago`;
  if (days < 30) return `${Math.floor(days / 7)}w ago`;
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: d.getFullYear() !== now.getFullYear() ? 'numeric' : undefined });
}
