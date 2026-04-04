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
    debounceTimer = setTimeout(() => {
      onSearch(inputValue);
    }, 300);
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

<div class="px-4 py-2.5 border-b border-gray-100 dark:border-gray-800 bg-gray-50/50 dark:bg-gray-800/30">
  <div class="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-white dark:bg-gray-800 border transition-colors
    {isFocused ? 'border-accent/40 shadow-sm' : 'border-gray-200 dark:border-gray-700'}">
    <svg class="w-4 h-4 shrink-0 transition-colors {isFocused ? 'text-accent' : 'text-gray-400 dark:text-gray-500'}" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" />
    </svg>
    <input
      bind:this={inputEl}
      type="text"
      placeholder="Search saved content..."
      class="flex-1 text-sm outline-none bg-transparent text-gray-900 dark:text-gray-100 placeholder-gray-400 dark:placeholder-gray-500"
      bind:value={inputValue}
      oninput={handleInput}
      onkeydown={handleKeydown}
      onfocus={() => isFocused = true}
      onblur={() => isFocused = false}
    />
    {#if inputValue}
      <button
        class="text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 p-0.5 rounded transition-colors"
        onclick={handleClear}
        aria-label="Clear search"
      >
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path d="M6 18L18 6M6 6l12 12" />
        </svg>
      </button>
    {:else if !isFocused}
      <kbd class="text-[10px] px-1.5 py-0.5 rounded border border-gray-200 dark:border-gray-600 text-gray-400 dark:text-gray-500 font-mono">/</kbd>
    {/if}
  </div>
</div>
