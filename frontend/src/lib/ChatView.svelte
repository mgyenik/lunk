<script lang="ts">
  import { api, type Entry, type ChatMessage, type ChatSource, type ChatResponseEvent, type LlmStatus } from '../api';
  import { listen } from '@tauri-apps/api/event';

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
  let messagesEndEl: HTMLDivElement | undefined = $state();

  const hasModel = $derived(llmStatus?.model_loaded ?? false);

  async function loadStatus() {
    try {
      llmStatus = await api.getLlmStatus();
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

    // Build history from prior messages (exclude the one we just added)
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
    messagesEndEl?.scrollIntoView({ behavior: 'smooth' });
  }

  function useExample(text: string) {
    inputValue = text;
  }

  // Listen for streaming tokens
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
</script>

<div class="flex-1 flex flex-col min-h-0 bg-surface">
  <!-- Header -->
  <div class="px-5 py-3 border-b border-border-subtle flex items-center gap-3 shrink-0">
    <h2 class="text-[14px] font-semibold text-text-primary">Chat</h2>
    <span class="font-brand text-[11px] text-text-tertiary">Ask your archive</span>
    <div class="flex-1"></div>
    {#if messages.length > 0}
      <button
        class="text-[11px] px-2.5 py-1 rounded-md border border-border text-text-secondary hover:bg-surface-raised transition-colors"
        onclick={newConversation}
      >New conversation</button>
    {/if}
  </div>

  <!-- Messages -->
  <div class="flex-1 overflow-y-auto p-4 space-y-4">
    {#if !hasModel}
      <!-- No model state -->
      <div class="flex flex-col items-center justify-center py-20">
        <div class="w-12 h-12 rounded-xl bg-accent/10 flex items-center justify-center mb-3">
          <svg class="w-6 h-6 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
          </svg>
        </div>
        <p class="text-[13px] text-text-secondary mb-1">No AI model loaded</p>
        <p class="text-[11px] text-text-tertiary">Download a model in Settings to start chatting</p>
      </div>
    {:else if messages.length === 0 && !isStreaming}
      <!-- Empty state -->
      <div class="flex flex-col items-center justify-center py-16">
        <div class="w-12 h-12 rounded-xl bg-accent/10 flex items-center justify-center mb-3">
          <svg class="w-6 h-6 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
          </svg>
        </div>
        <p class="text-[14px] font-medium text-text-primary mb-1">Ask your archive</p>
        <p class="text-[12px] text-text-tertiary mb-4 max-w-sm text-center">
          Ask questions about your saved articles and PDFs. Answers are grounded in your content with source citations.
        </p>
        <div class="flex flex-wrap gap-2 justify-center max-w-md">
          {#each [
            "What approaches exist for single-cell impedance?",
            "Summarize the MP6002 datasheet",
            "Compare the digital filter papers I've saved",
          ] as example}
            <button
              class="text-[11px] px-3 py-1.5 rounded-lg border border-border-subtle text-text-secondary
                hover:border-accent/30 hover:text-accent transition-colors text-left"
              onclick={() => useExample(example)}
            >{example}</button>
          {/each}
        </div>
      </div>
    {:else}
      <!-- Message list -->
      {#each messages as msg, i}
        <div class="flex {msg.role === 'user' ? 'justify-end' : 'justify-start'}">
          <div class="max-w-[80%] {msg.role === 'user'
            ? 'bg-accent text-white rounded-2xl rounded-br-md px-4 py-2.5'
            : 'bg-surface-raised border border-border rounded-2xl rounded-bl-md px-4 py-3'}">
            <p class="text-[13px] leading-relaxed whitespace-pre-wrap">{msg.content}</p>
          </div>
        </div>
        <!-- Source cards for assistant messages -->
        {#if msg.role === 'assistant' && msg.sources && msg.sources.length > 0}
          <div class="flex gap-2 overflow-x-auto pb-1 pl-1">
            {#each msg.sources as source}
              <button
                class="shrink-0 px-3 py-2 rounded-lg bg-surface-raised border border-border-subtle text-left
                  hover:border-accent/30 transition-all max-w-[220px]"
                onclick={() => handleSourceClick(source)}
              >
                <div class="flex items-center gap-1.5 mb-1">
                  <span class="text-[9px] font-brand px-1 py-0.5 rounded bg-accent-soft text-accent">{source.label}</span>
                  <span class="text-[11px] font-medium text-text-primary truncate">{source.entry_title}</span>
                </div>
                <p class="text-[10px] text-text-tertiary line-clamp-2">{source.snippet}</p>
              </button>
            {/each}
          </div>
        {/if}
      {/each}

      <!-- Streaming response -->
      {#if isStreaming}
        <div class="flex justify-start">
          <div class="max-w-[80%] bg-surface-raised border border-border rounded-2xl rounded-bl-md px-4 py-3">
            {#if streamContent}
              <p class="text-[13px] leading-relaxed whitespace-pre-wrap">{streamContent}</p>
            {:else}
              <div class="flex items-center gap-1.5">
                <div class="w-1.5 h-1.5 rounded-full bg-accent animate-pulse"></div>
                <div class="w-1.5 h-1.5 rounded-full bg-accent animate-pulse" style="animation-delay: 0.2s"></div>
                <div class="w-1.5 h-1.5 rounded-full bg-accent animate-pulse" style="animation-delay: 0.4s"></div>
              </div>
            {/if}
          </div>
        </div>
        <!-- Streaming source cards -->
        {#if streamSources.length > 0}
          <div class="flex gap-2 overflow-x-auto pb-1 pl-1">
            {#each streamSources as source}
              <button
                class="shrink-0 px-3 py-2 rounded-lg bg-surface-raised border border-border-subtle text-left
                  hover:border-accent/30 transition-all max-w-[220px]"
                onclick={() => handleSourceClick(source)}
              >
                <div class="flex items-center gap-1.5 mb-1">
                  <span class="text-[9px] font-brand px-1 py-0.5 rounded bg-accent-soft text-accent">{source.label}</span>
                  <span class="text-[11px] font-medium text-text-primary truncate">{source.entry_title}</span>
                </div>
                <p class="text-[10px] text-text-tertiary line-clamp-2">{source.snippet}</p>
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    {/if}

    <div bind:this={messagesEndEl}></div>
  </div>

  {#if error}
    <div class="mx-4 mb-2 px-3 py-2 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg text-[12px] text-red-700 dark:text-red-400">
      {error}
    </div>
  {/if}

  <!-- Input -->
  {#if hasModel}
    <div class="px-4 pb-4 pt-2 shrink-0">
      <div class="flex items-end gap-2 bg-surface-raised border border-border rounded-xl px-3 py-2
        focus-within:border-accent/40 focus-within:shadow-sm focus-within:shadow-accent/5 transition-all">
        <textarea
          class="flex-1 bg-transparent text-[13px] text-text-primary placeholder-text-tertiary
            outline-none resize-none max-h-32 leading-relaxed"
          rows="1"
          placeholder="Ask a question about your archive..."
          bind:value={inputValue}
          onkeydown={handleKeydown}
          disabled={isStreaming}
        ></textarea>
        <button
          class="shrink-0 w-8 h-8 rounded-lg flex items-center justify-center transition-colors
            {inputValue.trim() && !isStreaming
              ? 'bg-accent text-white hover:bg-accent-hover'
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
