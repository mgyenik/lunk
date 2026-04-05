<script lang="ts">
  interface Props {
    value: string;
    onSearch: (query: string) => void;
  }
  let { value, onSearch }: Props = $props();

  let inputEl = $state<HTMLInputElement | undefined>(undefined);
  let inputValue = $state('');
  let isFocused = $state(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    inputValue = value;
  });

  function handleGlobalKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (e.key === '/' && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      inputEl?.focus();
    }
  }

  function handleInput(e: Event) {
    const target = e.target as HTMLInputElement;
    inputValue = target.value;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => { onSearch(inputValue); }, 300);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      if (debounceTimer) clearTimeout(debounceTimer);
      onSearch(inputValue);
    }
    if (e.key === 'Escape') {
      inputValue = '';
      onSearch('');
      inputEl?.blur();
    }
  }

  function handleClear() {
    inputValue = '';
    onSearch('');
    inputEl?.focus();
  }

  export function focus() {
    inputEl?.focus();
  }
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="px-4 py-3 bg-surface border-b border-border">
  <div class="flex items-center gap-2.5 px-3 py-[7px] rounded-lg bg-surface-raised border transition-all duration-200
    {isFocused ? 'border-accent/30 shadow-[0_0_0_3px_rgba(134,59,255,0.06)]' : 'border-border'}">
    <svg class="w-[15px] h-[15px] shrink-0 transition-colors duration-200
      {isFocused ? 'text-accent' : 'text-text-tertiary'}" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" />
    </svg>
    <input
      bind:this={inputEl}
      type="text"
      placeholder="Search your archive..."
      class="flex-1 text-base outline-none bg-transparent text-text-primary placeholder-text-tertiary"
      bind:value={inputValue}
      oninput={handleInput}
      onkeydown={handleKeydown}
      onfocus={() => isFocused = true}
      onblur={() => isFocused = false}
    />
    {#if inputValue}
      <button
        class="text-text-tertiary hover:text-text-secondary p-0.5 rounded transition-colors"
        onclick={handleClear}
        aria-label="Clear search"
      >
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path d="M6 18L18 6M6 6l12 12" />
        </svg>
      </button>
    {:else if !isFocused}
      <kbd class="font-brand text-sm px-1.5 py-0.5 rounded border border-border text-text-tertiary">/</kbd>
    {/if}
  </div>
</div>
