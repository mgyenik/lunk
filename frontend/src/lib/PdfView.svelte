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
      await tick(); // wait for canvas element to mount
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
  <div class="flex items-center gap-2 px-3 py-1.5 bg-gray-100 dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 text-xs shrink-0">
    <button
      class="px-1.5 py-0.5 rounded hover:bg-gray-200 dark:hover:bg-gray-700 disabled:opacity-30 disabled:cursor-default text-gray-700 dark:text-gray-300"
      disabled={currentPage <= 1}
      onclick={() => goTo(currentPage - 1)}
      title="Previous page (Left arrow)"
    >
      &#8249;
    </button>
    <div class="flex items-center gap-1">
      <input
        type="text"
        class="w-10 text-center rounded border border-gray-300 dark:border-gray-600 py-0.5 text-xs bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100"
        bind:value={pageInputValue}
        onkeydown={handlePageKeydown}
      />
      <span class="text-gray-500 dark:text-gray-400">/ {totalPages}</span>
    </div>
    <button
      class="px-1.5 py-0.5 rounded hover:bg-gray-200 dark:hover:bg-gray-700 disabled:opacity-30 disabled:cursor-default text-gray-700 dark:text-gray-300"
      disabled={currentPage >= totalPages}
      onclick={() => goTo(currentPage + 1)}
      title="Next page (Right arrow)"
    >
      &#8250;
    </button>

    <div class="w-px h-4 bg-gray-300 dark:bg-gray-600 mx-1"></div>

    <button
      class="px-1.5 py-0.5 rounded hover:bg-gray-200 dark:hover:bg-gray-700 disabled:opacity-30 text-gray-700 dark:text-gray-300"
      disabled={scale <= 0.5}
      onclick={() => zoom(-0.25)}
      title="Zoom out (-)"
    >
      &minus;
    </button>
    <span class="w-12 text-center text-gray-600 dark:text-gray-400">{Math.round(scale * 100)}%</span>
    <button
      class="px-1.5 py-0.5 rounded hover:bg-gray-200 dark:hover:bg-gray-700 disabled:opacity-30 text-gray-700 dark:text-gray-300"
      disabled={scale >= 4}
      onclick={() => zoom(0.25)}
      title="Zoom in (+)"
    >
      +
    </button>

    <button
      class="px-2 py-0.5 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-600 dark:text-gray-400"
      onclick={fitWidth}
      title="Fit to width"
    >
      Fit
    </button>
  </div>

  <!-- Canvas area -->
  <div bind:this={scrollEl} class="flex-1 overflow-auto bg-gray-300 dark:bg-gray-700">
    {#if isLoading}
      <div class="flex items-center justify-center h-full text-gray-500 dark:text-gray-400 text-sm">
        Loading PDF...
      </div>
    {:else}
      <div class="flex justify-center p-4">
        <canvas bind:this={canvasEl} class="shadow-lg bg-white"></canvas>
      </div>
    {/if}
  </div>
</div>
