<script lang="ts">
  /**
   * Interactive metadata chip used for tags, domains, keywords, and content types.
   * Clicking navigates to a filtered view. Consistent across all views.
   *
   * Variants:
   *   tag     — #name, accent-soft background
   *   domain  — favicon + name, surface background
   *   keyword — name, surface-sunken background
   *   type    — PDF/WEB, colored
   */
  interface Props {
    label: string;
    variant?: 'tag' | 'domain' | 'keyword' | 'type';
    /** Show a small × button for removable filters */
    removable?: boolean;
    /** Active/selected state */
    active?: boolean;
    /** Count badge (for tag lists) */
    count?: number;
    onclick?: () => void;
    onremove?: () => void;
  }
  let { label, variant = 'tag', removable = false, active = false, count, onclick, onremove }: Props = $props();

  const classes = $derived({
    tag: active
      ? 'bg-accent text-white'
      : 'bg-accent-soft text-accent hover:bg-accent hover:text-white',
    domain: active
      ? 'bg-accent text-white'
      : 'bg-surface-raised border border-border-subtle text-text-secondary hover:border-accent/40 hover:text-accent',
    keyword: active
      ? 'bg-accent text-white'
      : 'bg-surface-sunken text-text-tertiary hover:text-accent hover:bg-accent-soft',
    type: active
      ? 'bg-accent text-white'
      : 'bg-surface-raised border border-border-subtle text-text-secondary hover:border-accent/40',
  });

  function handleClick(e: MouseEvent) {
    e.stopPropagation();
    onclick?.();
  }

  function handleRemove(e: MouseEvent) {
    e.stopPropagation();
    onremove?.();
  }
</script>

<button
  class="inline-flex items-center gap-1 font-brand text-sm px-2 py-[3px] rounded-md
    cursor-pointer transition-all duration-150 select-none shrink-0
    {classes[variant]}"
  onclick={handleClick}
  title={label}
>
  {#if variant === 'tag'}
    <span class="opacity-60">#</span>{label}
  {:else if variant === 'domain'}
    <img
      src="https://www.google.com/s2/favicons?domain={label}&sz=16"
      alt="" class="w-3 h-3 rounded-sm"
      onerror={(e) => { (e.target as HTMLImageElement).style.display = 'none'; }}
    />
    {label}
  {:else}
    {label}
  {/if}

  {#if count !== undefined}
    <span class="opacity-50 tabular-nums">{count}</span>
  {/if}

  {#if removable}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <span
      class="ml-0.5 opacity-40 hover:opacity-100 transition-opacity cursor-pointer"
      role="button"
      tabindex="0"
      onclick={handleRemove}
      onkeydown={(e) => { if (e.key === 'Enter') handleRemove(e as any); }}
    >&times;</span>
  {/if}
</button>
