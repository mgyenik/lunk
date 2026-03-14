<script lang="ts">
  interface Props {
    value: string;
    onSearch: (query: string) => void;
  }
  let { value, onSearch }: Props = $props();

  let inputEl = $state<HTMLInputElement | undefined>(undefined);
  let inputValue = $state(value);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // Sync external value changes
  $effect(() => {
    inputValue = value;
  });

  // Global keyboard shortcut: "/" to focus search
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
  }

  export function focus() {
    inputEl?.focus();
  }
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="flex items-center border-b border-gray-200 dark:border-gray-700 px-4 py-2 gap-2 bg-white dark:bg-gray-900">
  <svg class="w-4 h-4 text-gray-400 dark:text-gray-500 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
    <circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" />
  </svg>
  <input
    bind:this={inputEl}
    type="text"
    placeholder="Search saved content... (press /)"
    class="flex-1 text-sm outline-none bg-transparent text-gray-900 dark:text-gray-100 placeholder-gray-400 dark:placeholder-gray-500"
    bind:value={inputValue}
    oninput={handleInput}
    onkeydown={handleKeydown}
  />
  {#if inputValue}
    <button
      class="text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 text-xs"
      onclick={handleClear}
    >
      Clear
    </button>
  {/if}
</div>
