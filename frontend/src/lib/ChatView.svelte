<script lang="ts">
  import { api, type Entry, type ChatMessage, type ChatSource, type ChatResponseEvent, type LlmStatus } from '../api';
  import { listen } from '@tauri-apps/api/event';
  import { fly, fade } from 'svelte/transition';

  interface Props {
    onNavigateToEntry: (entry: Entry) => void;
  }
  let { onNavigateToEntry }: Props = $props();

  interface DisplayMessage {
    role: 'user' | 'assistant';
    content: string;
    sources?: ChatSource[];
  }

  let messages = $state<DisplayMessage[]>([]);
  let inputValue = $state('');
  let isStreaming = $state(false);
  let streamContent = $state('');
  let streamSources = $state<ChatSource[]>([]);
  let sessionId = $state(crypto.randomUUID());
  let error = $state('');
  let llmStatus = $state<LlmStatus | null>(null);
  let suggestedQuestions = $state<string[]>([]);
  let messagesEndEl: HTMLDivElement | undefined = $state();
  let inputEl: HTMLTextAreaElement | undefined = $state();

  const hasModel = $derived(llmStatus?.model_loaded ?? false);

  async function loadStatus() {
    try {
      llmStatus = await api.getLlmStatus();
    } catch { /* ignore */ }
  }

  async function loadSuggestions() {
    try {
      const suggestions: string[] = [];
      const templates = [
        (t: string) => `What does "${t}" cover?`,
        (t: string) => `Summarize "${t}"`,
        (t: string) => `What are the key points in "${t}"?`,
      ];

      const recent = await api.listEntries({ limit: 6 });
      const titles = recent.entries
        .map(e => e.title)
        .filter(t => t.length > 10 && t.length < 80);

      for (let i = 0; i < Math.min(2, titles.length); i++) {
        suggestions.push(templates[i % templates.length](titles[i]));
      }

      const tags = await api.getTags();
      const topTags = tags.filter(t => t.count >= 2).slice(0, 3);
      for (const tag of topTags.slice(0, 1)) {
        suggestions.push(`What have I saved about ${tag.name}?`);
      }

      if (suggestions.length === 0) {
        suggestions.push("What topics does my archive cover?");
      }

      suggestedQuestions = suggestions;
    } catch { /* ignore */ }
  }

  async function sendMessage() {
    const text = inputValue.trim();
    if (!text || isStreaming) return;

    error = '';
    messages.push({ role: 'user', content: text });
    inputValue = '';
    isStreaming = true;
    streamContent = '';
    streamSources = [];
    scrollToBottom();

    const history: ChatMessage[] = messages.slice(0, -1).map(m => ({
      role: m.role,
      content: m.content,
    }));

    try {
      await api.sendChatMessage(text, history, sessionId);
    } catch (err) {
      error = `${err}`;
      isStreaming = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }

  function newConversation() {
    messages = [];
    streamContent = '';
    streamSources = [];
    error = '';
    sessionId = crypto.randomUUID();
    inputEl?.focus();
  }

  async function handleSourceClick(source: ChatSource) {
    try {
      const entry = await api.getEntry(source.entry_id);
      onNavigateToEntry(entry);
    } catch (err) {
      error = `Failed to load entry: ${err}`;
    }
  }

  function scrollToBottom() {
    requestAnimationFrame(() => {
      messagesEndEl?.scrollIntoView({ behavior: 'smooth' });
    });
  }

  function useExample(text: string) {
    inputValue = text;
    inputEl?.focus();
  }

  // Parse citation markers [1], [2] etc. into segments for rendering
  function parseCitations(text: string): Array<{ type: 'text' | 'citation'; value: string }> {
    const parts: Array<{ type: 'text' | 'citation'; value: string }> = [];
    const regex = /\[(\d+)\]/g;
    let lastIndex = 0;
    let match;
    while ((match = regex.exec(text)) !== null) {
      if (match.index > lastIndex) {
        parts.push({ type: 'text', value: text.slice(lastIndex, match.index) });
      }
      parts.push({ type: 'citation', value: match[0] });
      lastIndex = regex.lastIndex;
    }
    if (lastIndex < text.length) {
      parts.push({ type: 'text', value: text.slice(lastIndex) });
    }
    return parts.length > 0 ? parts : [{ type: 'text', value: text }];
  }

  $effect(() => {
    const unlisten = listen<ChatResponseEvent>('chat-response', (event) => {
      const p = event.payload;
      if (p.session_id !== sessionId) return;

      if (p.sources) {
        streamSources = p.sources;
      }

      if (p.token) {
        streamContent += p.token;
        scrollToBottom();
      }

      if (p.done) {
        messages.push({
          role: 'assistant',
          content: streamContent,
          sources: streamSources.length > 0 ? [...streamSources] : undefined,
        });
        streamContent = '';
        streamSources = [];
        isStreaming = false;
        scrollToBottom();
      }
    });
    return () => { unlisten.then(fn => fn()); };
  });

  loadStatus();
  loadSuggestions();
</script>

<div class="flex-1 flex flex-col min-h-0 bg-surface">
  <!-- Header -->
  <div class="px-5 py-3 border-b border-border-subtle flex items-center gap-3 shrink-0">
    <svg class="w-4 h-4 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8">
      <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
    </svg>
    <h2 class="text-md font-semibold text-text-primary">Chat</h2>
    <div class="flex-1"></div>
    {#if messages.length > 0}
      <button
        class="text-ui px-2.5 py-1 rounded-md border border-border text-text-secondary hover:bg-surface-raised hover:text-text-primary transition-colors"
        onclick={newConversation}
      >New conversation</button>
    {/if}
  </div>

  <!-- Messages -->
  <div class="flex-1 overflow-y-auto p-4 space-y-4" aria-live="polite" aria-label="Chat messages">
    {#if !hasModel}
      <div class="flex flex-col items-center justify-center py-20" in:fade={{ duration: 150 }}>
        <div class="w-14 h-14 rounded-2xl bg-surface-sunken flex items-center justify-center mb-4">
          <svg class="w-7 h-7 text-text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
          </svg>
        </div>
        <p class="text-base text-text-secondary mb-1">No AI model loaded</p>
        <p class="text-ui text-text-tertiary">Download a model in Settings to start chatting</p>
      </div>
    {:else if messages.length === 0 && !isStreaming}
      <div class="flex flex-col items-center justify-center py-16" in:fade={{ duration: 150 }}>
        <div class="w-14 h-14 rounded-2xl bg-accent/8 flex items-center justify-center mb-4">
          <svg class="w-7 h-7 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
          </svg>
        </div>
        <p class="text-md font-medium text-text-primary mb-1">Ask your archive</p>
        <p class="text-body text-text-tertiary mb-5 max-w-sm text-center leading-relaxed">
          Ask questions about your saved articles and PDFs. Answers are grounded in your content with source citations.
        </p>
        <div class="flex flex-wrap gap-2 justify-center max-w-md">
          {#each suggestedQuestions as example}
            <button
              class="text-ui px-3 py-1.5 rounded-lg border border-border-subtle text-text-secondary
                hover:border-accent/30 hover:text-accent hover:bg-accent-soft/30 transition-all text-left"
              onclick={() => useExample(example)}
            >{example}</button>
          {/each}
        </div>
      </div>
    {:else}
      {#each messages as msg, i}
        <!-- User message -->
        {#if msg.role === 'user'}
          <div class="flex justify-end" in:fly={{ y: 8, duration: 200 }}>
            <div class="max-w-[80%] bg-accent text-white rounded-2xl rounded-br-sm px-4 py-2.5 shadow-sm shadow-accent/10">
              <p class="text-base leading-relaxed whitespace-pre-wrap">{msg.content}</p>
            </div>
          </div>
        {:else}
          <!-- Assistant message with inline citations -->
          <div class="flex justify-start" in:fly={{ y: 8, duration: 200 }}>
            <div class="max-w-[85%] bg-surface-raised border border-border-subtle rounded-2xl rounded-bl-sm px-4 py-3">
              <p class="text-base leading-relaxed whitespace-pre-wrap">
                {#each parseCitations(msg.content) as part}
                  {#if part.type === 'citation'}
                    <button
                      class="inline-flex items-center px-1 py-0.5 -my-0.5 mx-0.5 rounded text-sm font-brand
                        bg-accent-soft text-accent hover:bg-accent hover:text-white transition-colors cursor-pointer align-baseline"
                      onclick={() => {
                        const idx = parseInt(part.value.slice(1, -1)) - 1;
                        if (msg.sources?.[idx]) handleSourceClick(msg.sources[idx]);
                      }}
                    >{part.value}</button>
                  {:else}
                    {part.value}
                  {/if}
                {/each}
              </p>
            </div>
          </div>
          <!-- Source cards -->
          {#if msg.sources && msg.sources.length > 0}
            <div class="flex gap-2 overflow-x-auto pb-1 pl-1 scroll-x-hidden" in:fade={{ duration: 150, delay: 100 }}>
              {#each msg.sources as source}
                <button
                  class="shrink-0 px-3 py-2 rounded-lg bg-surface-raised border border-border-subtle text-left
                    hover:border-accent/30 hover:shadow-sm transition-all max-w-[220px] group"
                  onclick={() => handleSourceClick(source)}
                >
                  <div class="flex items-center gap-1.5 mb-1">
                    <span class="text-xs font-brand px-1 py-0.5 rounded bg-accent-soft text-accent">{source.label}</span>
                    <span class="text-ui font-medium text-text-primary truncate group-hover:text-accent transition-colors">{source.entry_title}</span>
                  </div>
                  <p class="text-sm text-text-tertiary line-clamp-2">{source.snippet}</p>
                </button>
              {/each}
            </div>
          {/if}
        {/if}
      {/each}

      <!-- Streaming response -->
      {#if isStreaming}
        <div class="flex justify-start" in:fly={{ y: 8, duration: 200 }}>
          <div class="max-w-[85%] bg-surface-raised border border-border-subtle rounded-2xl rounded-bl-sm px-4 py-3">
            {#if streamContent}
              <p class="text-base leading-relaxed whitespace-pre-wrap">
                {#each parseCitations(streamContent) as part}
                  {#if part.type === 'citation'}
                    <span class="inline-flex items-center px-1 py-0.5 -my-0.5 mx-0.5 rounded text-sm font-brand bg-accent-soft text-accent align-baseline">{part.value}</span>
                  {:else}
                    {part.value}
                  {/if}
                {/each}
                <span class="inline-block w-0.5 h-4 bg-accent/60 animate-pulse ml-0.5 align-text-bottom"></span>
              </p>
            {:else}
              <div class="flex items-center gap-1.5 py-1">
                <span class="typing-dot"></span>
                <span class="typing-dot delay-1"></span>
                <span class="typing-dot delay-2"></span>
              </div>
            {/if}
          </div>
        </div>
        {#if streamSources.length > 0}
          <div class="flex gap-2 overflow-x-auto pb-1 pl-1 scroll-x-hidden" in:fade={{ duration: 150 }}>
            {#each streamSources as source}
              <button
                class="shrink-0 px-3 py-2 rounded-lg bg-surface-raised border border-border-subtle text-left
                  hover:border-accent/30 hover:shadow-sm transition-all max-w-[220px] group"
                onclick={() => handleSourceClick(source)}
              >
                <div class="flex items-center gap-1.5 mb-1">
                  <span class="text-xs font-brand px-1 py-0.5 rounded bg-accent-soft text-accent">{source.label}</span>
                  <span class="text-ui font-medium text-text-primary truncate group-hover:text-accent transition-colors">{source.entry_title}</span>
                </div>
                <p class="text-sm text-text-tertiary line-clamp-2">{source.snippet}</p>
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    {/if}

    <div bind:this={messagesEndEl}></div>
  </div>

  {#if error}
    <div class="mx-4 mb-2 px-3 py-2 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg text-body text-red-700 dark:text-red-400"
      in:fly={{ y: 4, duration: 150 }}>
      {error}
    </div>
  {/if}

  <!-- Input -->
  {#if hasModel}
    <div class="px-4 pb-4 pt-2 shrink-0">
      <div class="flex items-end gap-2 bg-surface-raised border border-border rounded-xl px-3 py-2
        focus-within:border-accent/40 focus-within:shadow-sm focus-within:shadow-accent/5 transition-all">
        <textarea
          bind:this={inputEl}
          class="flex-1 bg-transparent text-base text-text-primary placeholder-text-tertiary
            outline-none resize-none max-h-32 leading-relaxed"
          rows="1"
          placeholder="Ask a question about your archive..."
          bind:value={inputValue}
          onkeydown={handleKeydown}
          disabled={isStreaming}
        ></textarea>
        <button
          class="shrink-0 w-8 h-8 rounded-lg flex items-center justify-center transition-all duration-150
            {inputValue.trim() && !isStreaming
              ? 'bg-accent text-white hover:bg-accent-hover shadow-sm shadow-accent/20'
              : 'bg-surface-sunken text-text-tertiary cursor-not-allowed'}"
          onclick={sendMessage}
          disabled={!inputValue.trim() || isStreaming}
          aria-label="Send message"
        >
          <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path d="M5 12h14M12 5l7 7-7 7" />
          </svg>
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .typing-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background-color: var(--color-accent);
    opacity: 0.4;
    animation: typing-bounce 1.4s ease-in-out infinite;
  }
  .typing-dot.delay-1 { animation-delay: 0.2s; }
  .typing-dot.delay-2 { animation-delay: 0.4s; }

  @keyframes typing-bounce {
    0%, 60%, 100% { transform: translateY(0); opacity: 0.4; }
    30% { transform: translateY(-4px); opacity: 1; }
  }
</style>
