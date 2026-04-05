<script lang="ts">
  type ViewType = 'home' | 'browse' | 'search' | 'detail' | 'sync' | 'settings' | 'chat';

  interface Props {
    currentView: ViewType;
    onNavigate: (view: 'home' | 'sync' | 'settings' | 'chat') => void;
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

  const navBtn = "w-9 h-9 rounded-lg flex items-center justify-center transition-all";
  const navIcon = "w-[18px] h-[18px]";
</script>

<nav class="w-[52px] bg-surface-sunken border-r border-border flex flex-col items-center py-3 gap-1 shrink-0 relative texture-noise" aria-label="Main navigation">
  <!-- Logo -->
  <button
    class="w-9 h-9 rounded-xl bg-accent flex items-center justify-center shadow-sm shadow-accent/20 mb-3 hover:shadow-md hover:shadow-accent/30 transition-all"
    onclick={() => onNavigate('home')}
    aria-label="Home"
  >
    <svg class="w-4.5 h-4.5 text-white" viewBox="0 0 48 46" fill="currentColor">
      <path d="M25.946 44.938c-.664.845-2.021.375-2.021-.698V33.937a2.26 2.26 0 0 0-2.262-2.262H10.287c-.92 0-1.456-1.04-.92-1.788l7.48-10.471c1.07-1.497 0-3.578-1.842-3.578H1.237c-.92 0-1.456-1.04-.92-1.788L10.013.474c.214-.297.556-.474.92-.474h28.894c.92 0 1.456 1.04.92 1.788l-7.48 10.471c-1.07 1.498 0 3.579 1.842 3.579h11.377c.943 0 1.473 1.088.89 1.83L25.947 44.94z"/>
    </svg>
  </button>

  <!-- Home -->
  <button
    class="{navBtn} {isActive('home') ? 'bg-accent-soft text-accent' : 'text-text-tertiary hover:text-text-secondary hover:bg-surface-raised'}"
    onclick={() => onNavigate('home')}
    aria-label="Home"
    aria-current={isActive('home') ? 'page' : undefined}
  >
    <svg class={navIcon} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
      <path d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" />
    </svg>
  </button>

  <!-- Chat -->
  <button
    class="{navBtn} {isActive('chat') ? 'bg-accent-soft text-accent' : 'text-text-tertiary hover:text-text-secondary hover:bg-surface-raised'}"
    onclick={() => onNavigate('chat')}
    aria-label="Chat with archive"
    aria-current={isActive('chat') ? 'page' : undefined}
  >
    <svg class={navIcon} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
      <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
    </svg>
  </button>

  <!-- Sync -->
  <button
    class="{navBtn} {isActive('sync') ? 'bg-accent-soft text-accent' : 'text-text-tertiary hover:text-text-secondary hover:bg-surface-raised'}"
    onclick={() => onNavigate('sync')}
    aria-label="P2P Sync"
    aria-current={isActive('sync') ? 'page' : undefined}
  >
    <svg class={navIcon} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
      <path d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
    </svg>
  </button>

  <!-- Settings -->
  <button
    class="{navBtn} {isActive('settings') ? 'bg-accent-soft text-accent' : 'text-text-tertiary hover:text-text-secondary hover:bg-surface-raised'}"
    onclick={() => onNavigate('settings')}
    aria-label="Settings"
    aria-current={isActive('settings') ? 'page' : undefined}
  >
    <svg class={navIcon} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
      <path d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  </button>

  <div class="flex-1"></div>

  <!-- Import PDF -->
  <button
    class="w-9 h-9 rounded-lg bg-accent/10 text-accent hover:bg-accent hover:text-white flex items-center justify-center transition-all"
    onclick={onImportPdf}
    aria-label="Import PDF"
  >
    <svg class={navIcon} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <path d="M12 4v16m8-8H4" />
    </svg>
  </button>

  <!-- Dark mode toggle -->
  <button
    class="{navBtn} text-text-tertiary hover:text-text-secondary hover:bg-surface-raised"
    onclick={toggleDark}
    aria-label={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
  >
    {#if isDark}
      <svg class={navIcon} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
        <path d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
      </svg>
    {:else}
      <svg class={navIcon} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
        <path d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
      </svg>
    {/if}
  </button>

  <span class="font-brand text-micro text-text-tertiary mt-1">0.4</span>
</nav>
