<script lang="ts">
  /**
   * Persistent filter strip showing active filters as removable chips.
   * Appears when any filter is active. Command-line aesthetic.
   */
  import FilterChip from './FilterChip.svelte';

  interface Props {
    tag: string | null;
    domain: string | null;
    contentType: 'all' | 'article' | 'pdf';
    onClearTag: () => void;
    onClearDomain: () => void;
    onClearContentType: () => void;
    onClearAll: () => void;
  }
  let { tag, domain, contentType, onClearTag, onClearDomain, onClearContentType, onClearAll }: Props = $props();

  const hasFilters = $derived(tag !== null || domain !== null || contentType !== 'all');
</script>

{#if hasFilters}
  <div class="px-4 py-2 bg-surface-sunken border-b border-border flex items-center gap-2 shrink-0">
    <span class="font-brand text-xs uppercase tracking-[0.12em] text-text-tertiary">Filtering</span>

    {#if tag}
      <FilterChip label={tag} variant="tag" active removable onremove={onClearTag} />
    {/if}

    {#if domain}
      <FilterChip label={domain} variant="domain" active removable onremove={onClearDomain} />
    {/if}

    {#if contentType !== 'all'}
      <FilterChip
        label={contentType === 'pdf' ? 'PDFs' : 'Articles'}
        variant="type"
        active
        removable
        onremove={onClearContentType}
      />
    {/if}

    <button
      class="ml-auto font-brand text-xs text-text-tertiary hover:text-accent transition-colors"
      onclick={onClearAll}
    >Clear all</button>
  </div>
{/if}
