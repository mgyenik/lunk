<script lang="ts">
  type ViewType = 'home' | 'browse' | 'search' | 'detail' | 'sync';

  interface Props {
    currentView: ViewType;
    onNavigate: (view: 'home' | 'sync') => void;
    onImportPdf: () => void;
  }
  let { currentView, onNavigate, onImportPdf }: Props = $props();

  let isDark = $state(document.documentElement.classList.contains('dark'));

  function toggleDark() {
    isDark = !isDark;
    document.documentElement.classList.toggle('dark', isDark);
    localStorage.theme = isDark ? 'dark' : 'light';
  }

  function isActive(view: string): boolean {
    return currentView === view;
  }
</script>

<nav class="w-[52px] bg-surface-sunken border-r border-border flex flex-col items-center py-3 gap-1 shrink-0 relative texture-noise">
  <!-- Logo -->
  <button
    class="w-9 h-9 rounded-xl bg-accent flex items-center justify-center shadow-sm shadow-accent/20 mb-3 hover:shadow-md hover:shadow-accent/30 transition-all"
    onclick={() => onNavigate('home')}
    title="Home"
  >
    <svg class="w-4.5 h-4.5 text-white" viewBox="0 0 48 46" fill="currentColor">
      <path d="M25.946 44.938c-.664.845-2.021.375-2.021-.698V33.937a2.26 2.26 0 0 0-2.262-2.262H10.287c-.92 0-1.456-1.04-.92-1.788l7.48-10.471c1.07-1.497 0-3.578-1.842-3.578H1.237c-.92 0-1.456-1.04-.92-1.788L10.013.474c.214-.297.556-.474.92-.474h28.894c.92 0 1.456 1.04.92 1.788l-7.48 10.471c-1.07 1.498 0 3.579 1.842 3.579h11.377c.943 0 1.473 1.088.89 1.83L25.947 44.94z"/>
    </svg>
  </button>

  <!-- Home -->
  <button
    class="w-9 h-9 rounded-lg flex items-center justify-center transition-all
      {isActive('home') ? 'bg-accent-soft text-accent' : 'text-text-tertiary hover:text-text-secondary hover:bg-surface-raised'}"
    onclick={() => onNavigate('home')}
    title="Home"
  >
    <svg class="w-[18px] h-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
      <path d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" />
    </svg>
  </button>

  <!-- Sync -->
  <button
    class="w-9 h-9 rounded-lg flex items-center justify-center transition-all
      {isActive('sync') ? 'bg-accent-soft text-accent' : 'text-text-tertiary hover:text-text-secondary hover:bg-surface-raised'}"
    onclick={() => onNavigate('sync')}
    title="P2P Sync"
  >
    <svg class="w-[18px] h-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
      <path d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
    </svg>
  </button>

  <div class="flex-1"></div>

  <!-- Import PDF -->
  <button
    class="w-9 h-9 rounded-lg bg-accent/10 text-accent hover:bg-accent hover:text-white flex items-center justify-center transition-all"
    onclick={onImportPdf}
    title="Import PDF"
  >
    <svg class="w-[18px] h-[18px]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
      <path d="M12 4v16m8-8H4" />
    </svg>
  </button>

  <!-- Dark mode -->
  <button
    class="w-9 h-9 rounded-lg flex items-center justify-center text-text-tertiary hover:text-text-secondary hover:bg-surface-raised transition-all text-sm"
    onclick={toggleDark}
    title="Toggle dark mode"
  >
    {isDark ? '☀' : '☾'}
  </button>

  <span class="font-brand text-[8px] text-text-tertiary mt-1">0.2</span>
</nav>
