// Content script: runs in page context.
// Handles three extraction modes:
// 1. Readability extraction (clean text + readable HTML)
// 2. Full page snapshot via SingleFile (self-contained HTML archive)
// 3. PDF capture (fetch raw PDF bytes)

(function () {
  "use strict";

  // Listen for messages from background script
  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (msg.action === "extract") {
      handleExtract(msg.options || {})
        .then(sendResponse)
        .catch((err) => sendResponse({ error: err.message }));
      return true; // async response
    }
    if (msg.action === "ping") {
      sendResponse({ ok: true });
      return false;
    }
    // SingleFile cross-origin fetch fallback: background asks us to fetch
    if (msg.action === "singlefile.fetchResponse") {
      // Response from background for a cross-origin fetch we requested
      return false;
    }
  });

  async function handleExtract(options) {
    const result = {
      url: window.location.href,
      title: document.title,
      content_type: "article",
      extracted_text: null,
      readable_html: null,
      snapshot_html: null,
    };

    // Check if this is a PDF
    if (isPdfPage()) {
      result.content_type = "pdf";
      // Try URL filename if document.title is empty or generic
      const generic = ["", "pdf", "untitled", "document", "download"];
      if (!result.title || generic.includes(result.title.trim().toLowerCase()) || result.title.trim().length <= 3) {
        try {
          const path = new URL(window.location.href).pathname;
          const filename = decodeURIComponent(path.split("/").pop() || "");
          if (filename && !generic.includes(filename.toLowerCase())) {
            result.title = filename;
          }
        } catch {
          // keep whatever title we have
        }
      }
      const pdfData = await fetchPdfData();
      if (pdfData) {
        result.pdf_base64 = pdfData;
      } else {
        // Signal that background should try fetching the PDF itself
        result.needs_background_fetch = true;
      }
      return result;
    }

    // Step 1: Readability extraction (fast, ~50ms)
    try {
      const readabilityResult = extractReadability();
      result.title = readabilityResult.title || document.title;
      result.extracted_text = readabilityResult.textContent;
      result.readable_html = readabilityResult.content;
    } catch (err) {
      // Fallback: get body text
      result.extracted_text = document.body?.innerText || "";
      result.readable_html = null;
    }

    // Step 2: Full page snapshot via SingleFile
    if (options.skipSnapshot !== true) {
      try {
        const snapshot = await captureSnapshot();
        result.snapshot_html = snapshot;
      } catch (err) {
        console.warn("Grymoire: snapshot capture failed:", err);
        // Non-fatal: we still have readable content
      }
    }

    return result;
  }

  // --- Readability extraction ---

  function extractReadability() {
    // Clone the document so Readability doesn't mutate the live DOM
    const docClone = document.cloneNode(true);
    const reader = new Readability(docClone);
    const article = reader.parse();

    if (!article) {
      throw new Error("Readability could not parse this page");
    }

    return {
      title: article.title,
      content: article.content, // cleaned HTML
      textContent: article.textContent, // plain text
      excerpt: article.excerpt,
      byline: article.byline,
      siteName: article.siteName,
    };
  }

  // --- Full page snapshot via SingleFile ---

  async function captureSnapshot() {
    // SingleFile is loaded as a UMD global via manifest content_scripts
    if (typeof globalThis.singlefile === "undefined" || !globalThis.singlefile.getPageData) {
      throw new Error("SingleFile not loaded");
    }

    // Custom fetch that falls back to background service worker for cross-origin
    async function bgFetch(url, options) {
      try {
        const resp = await fetch(url, options);
        if (resp.ok) return resp;
        throw new Error(`HTTP ${resp.status}`);
      } catch {
        // Ask background to fetch cross-origin resource
        return new Promise((resolve, reject) => {
          chrome.runtime.sendMessage(
            { target: "background", action: "fetch_resource", url },
            (response) => {
              if (chrome.runtime.lastError) {
                reject(new Error(chrome.runtime.lastError.message));
                return;
              }
              if (!response || response.error) {
                reject(new Error(response?.error || "fetch failed"));
                return;
              }
              // Reconstruct a Response-like object from the base64 data
              const bytes = Uint8Array.from(atob(response.data), c => c.charCodeAt(0));
              const blob = new Blob([bytes], { type: response.contentType || "application/octet-stream" });
              resolve(new Response(blob, {
                status: 200,
                headers: { "Content-Type": response.contentType || "application/octet-stream" },
              }));
            }
          );
        });
      }
    }

    const pageData = await globalThis.singlefile.getPageData(
      {
        // Content cleanup
        removeHiddenElements: true,
        removeUnusedStyles: true,
        removeUnusedFonts: true,
        removeAlternativeFonts: true,
        removeAlternativeMedias: true,
        removeAlternativeImages: true,
        compressHTML: true,

        // Block active content
        blockScripts: true,
        blockVideos: false,
        blockAudios: true,

        // Lazy-loaded images
        loadDeferredImages: true,
        loadDeferredImagesMaxIdleTime: 1500,

        // Frames
        removeFrames: false,

        // Metadata
        insertCanonicalLink: true,
        insertMetaCSP: true,
        insertMetaNoIndex: false,
        insertSingleFileComment: true,

        // Deduplication
        groupDuplicateImages: true,

        // Resource limits — cap individual resources at 5MB to keep
        // total snapshot size reasonable (images are the main culprit)
        maxResourceSizeEnabled: true,
        maxResourceSize: 5,
      },
      { fetch: bgFetch, frameFetch: bgFetch }
    );

    let html = pageData.content;

    // Deduplicate <style> tags — web component frameworks (Reddit, etc.)
    // inject identical stylesheets hundreds of times.
    const seen = new Set();
    html = html.replace(/<style[\s\S]*?<\/style>/g, (match) => {
      if (seen.has(match)) return "";
      seen.add(match);
      return match;
    });

    console.log(`Grymoire: snapshot size: ${html.length} bytes (${Math.round(html.length/1024)}KB)`);

    return html;
  }

  // --- PDF handling ---

  function isPdfPage() {
    // Check content type or URL
    if (window.location.pathname.toLowerCase().endsWith(".pdf")) return true;
    const ct = document.contentType;
    if (ct && ct.includes("application/pdf")) return true;
    // Chrome's built-in PDF viewer
    if (document.querySelector("embed[type='application/pdf']")) return true;
    return false;
  }

  async function fetchPdfData() {
    try {
      const response = await fetch(window.location.href);
      const blob = await response.blob();
      return new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => {
          const base64 = reader.result.split(",")[1];
          resolve(base64);
        };
        reader.onerror = reject;
        reader.readAsDataURL(blob);
      });
    } catch {
      // file:// URLs can't be fetched from content scripts —
      // return null and let the background script handle it
      return null;
    }
  }
})();
