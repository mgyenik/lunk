<script lang="ts">
  import { tick } from 'svelte';
  import * as pdfjsLib from 'pdfjs-dist';

  pdfjsLib.GlobalWorkerOptions.workerSrc = '/pdf.worker.min.mjs';

  interface Props {
    data: string;
    initialPage?: number;
  }
  let { data, initialPage = 1 }: Props = $props();

  let canvasEl = $state<HTMLCanvasElement | undefined>(undefined);
  let scrollEl = $state<HTMLDivElement | undefined>(undefined);
  let pdfDoc: pdfjsLib.PDFDocumentProxy | null = null;
  let currentPage = $state(1);
  let totalPages = $state(0);
  let scale = $state(1.5);
  let pageInputValue = $state('1');
  let isLoading = $state(true);
  let renderInProgress = false;

  $effect(() => {
    loadPdf(data);
    return () => {
      pdfDoc?.destroy();
      pdfDoc = null;
    };
  });

  async function loadPdf(b64: string) {
    isLoading = true;
    pdfDoc?.destroy();
    pdfDoc = null;

    try {
      const raw = atob(b64);
      const bytes = new Uint8Array(raw.length);
      for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);

      pdfDoc = await pdfjsLib.getDocument({ data: bytes }).promise;
      totalPages = pdfDoc.numPages;
      const page = Math.max(1, Math.min(initialPage, totalPages));
      currentPage = page;
      pageInputValue = String(page);
      isLoading = false;
      await tick();
      await renderPage(page);
    } catch (err) {
      console.error('PDF load error:', err);
      isLoading = false;
    }
  }

  async function renderPage(num: number) {
    if (!pdfDoc || !canvasEl || renderInProgress) return;
    renderInProgress = true;

    try {
      const page = await pdfDoc.getPage(num);
      const viewport = page.getViewport({ scale });
      const dpr = window.devicePixelRatio || 1;
      const ctx = canvasEl.getContext('2d')!;

      canvasEl.width = Math.floor(viewport.width * dpr);
      canvasEl.height = Math.floor(viewport.height * dpr);
      canvasEl.style.width = `${viewport.width}px`;
      canvasEl.style.height = `${viewport.height}px`;

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      await page.render({ canvas: canvasEl, canvasContext: ctx, viewport }).promise;
    } catch (err) {
      console.error('Render error:', err);
    } finally {
      renderInProgress = false;
    }

    scrollEl?.scrollTo(0, 0);
  }

  function goTo(num: number) {
    if (num < 1 || num > totalPages) return;
    currentPage = num;
    pageInputValue = String(num);
    renderPage(num);
  }

  function handlePageKeydown(e: KeyboardEvent) {
    if (e.key !== 'Enter') return;
    const num = parseInt(pageInputValue);
    if (!isNaN(num) && num >= 1 && num <= totalPages) {
      goTo(num);
    } else {
      pageInputValue = String(currentPage);
    }
  }

  function zoom(delta: number) {
    const next = Math.max(0.5, Math.min(4, scale + delta));
    if (next === scale) return;
    scale = next;
    renderPage(currentPage);
  }

  function fitWidth() {
    if (!pdfDoc || !scrollEl) return;
    pdfDoc.getPage(currentPage).then(page => {
      const unscaled = page.getViewport({ scale: 1 });
      const containerWidth = scrollEl!.clientWidth - 32;
      scale = containerWidth / unscaled.width;
      renderPage(currentPage);
    });
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLInputElement) return;
    switch (e.key) {
      case 'ArrowLeft':
        goTo(currentPage - 1);
        break;
      case 'ArrowRight':
        goTo(currentPage + 1);
        break;
      case '+':
      case '=':
        zoom(0.25);
        break;
      case '-':
        zoom(-0.25);
        break;
      case 'Home':
        goTo(1);
        break;
      case 'End':
        goTo(totalPages);
        break;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex flex-col h-full">
  <!-- Toolbar -->
  <div class="flex items-center gap-2 px-3 py-1.5 bg-surface-sunken border-b border-border text-xs shrink-0">
    <button
      class="px-1.5 py-0.5 rounded hover:bg-surface-raised disabled:opacity-30 disabled:cursor-default text-text-secondary"
      disabled={currentPage <= 1}
      onclick={() => goTo(currentPage - 1)}
      aria-label="Previous page"
      title="Previous page (Left arrow)"
    >
      &#8249;
    </button>
    <div class="flex items-center gap-1">
      <input
        type="text"
        class="w-10 text-center rounded border border-border py-0.5 text-xs bg-surface-raised text-text-primary"
        bind:value={pageInputValue}
        onkeydown={handlePageKeydown}
        aria-label="Page number"
      />
      <span class="text-text-tertiary font-brand">/ {totalPages}</span>
    </div>
    <button
      class="px-1.5 py-0.5 rounded hover:bg-surface-raised disabled:opacity-30 disabled:cursor-default text-text-secondary"
      disabled={currentPage >= totalPages}
      onclick={() => goTo(currentPage + 1)}
      aria-label="Next page"
      title="Next page (Right arrow)"
    >
      &#8250;
    </button>

    <div class="w-px h-4 bg-border mx-1"></div>

    <button
      class="px-1.5 py-0.5 rounded hover:bg-surface-raised disabled:opacity-30 text-text-secondary"
      disabled={scale <= 0.5}
      onclick={() => zoom(-0.25)}
      aria-label="Zoom out"
      title="Zoom out (-)"
    >
      &minus;
    </button>
    <span class="w-12 text-center text-text-tertiary font-brand">{Math.round(scale * 100)}%</span>
    <button
      class="px-1.5 py-0.5 rounded hover:bg-surface-raised disabled:opacity-30 text-text-secondary"
      disabled={scale >= 4}
      onclick={() => zoom(0.25)}
      aria-label="Zoom in"
      title="Zoom in (+)"
    >
      +
    </button>

    <button
      class="px-2 py-0.5 rounded hover:bg-surface-raised text-text-tertiary"
      onclick={fitWidth}
      title="Fit to width"
    >
      Fit
    </button>
  </div>

  <!-- Canvas area -->
  <div bind:this={scrollEl} class="flex-1 overflow-auto bg-surface-sunken">
    {#if isLoading}
      <div class="flex items-center justify-center h-full">
        <div class="w-5 h-5 rounded-full border-2 border-accent/20 border-t-accent animate-spin"></div>
      </div>
    {:else}
      <div class="flex justify-center p-4">
        <canvas bind:this={canvasEl} class="shadow-lg bg-white dark:bg-white"></canvas>
      </div>
    {/if}
  </div>
</div>
