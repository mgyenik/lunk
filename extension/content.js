// Content script: runs in page context.
// Handles two extraction modes:
// 1. Readability extraction (clean text + readable HTML)
// 2. Full page snapshot (self-contained HTML with inlined assets)

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
      try {
        const pdfData = await fetchPdfData();
        result.pdf_base64 = pdfData;
      } catch (err) {
        result.error = `PDF capture failed: ${err.message}`;
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

    // Step 2: Full page snapshot (slower, 1-5s)
    if (options.skipSnapshot !== true) {
      try {
        const snapshot = await captureSnapshot();
        result.snapshot_html = snapshot;
      } catch (err) {
        console.warn("Lunk: snapshot capture failed:", err);
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

  // --- Full page snapshot ---

  async function captureSnapshot() {
    const clone = document.documentElement.cloneNode(true);

    // Remove scripts
    clone.querySelectorAll("script, noscript").forEach((el) => el.remove());

    // Remove event handler attributes
    const allElements = clone.querySelectorAll("*");
    for (const el of allElements) {
      const attrs = Array.from(el.attributes);
      for (const attr of attrs) {
        if (attr.name.startsWith("on")) {
          el.removeAttribute(attr.name);
        }
      }
    }

    // Inline images as data URIs
    const images = clone.querySelectorAll("img[src]");
    const imagePromises = Array.from(images).map(async (img) => {
      try {
        const src = img.getAttribute("src");
        if (!src || src.startsWith("data:")) return;

        const absoluteUrl = new URL(src, window.location.href).href;
        const dataUri = await fetchAsDataUri(absoluteUrl);
        if (dataUri) {
          img.setAttribute("src", dataUri);
        }
      } catch {
        // Keep original src on failure
      }
    });

    // Also handle srcset
    const srcsetImages = clone.querySelectorAll("img[srcset], source[srcset]");
    for (const el of srcsetImages) {
      el.removeAttribute("srcset");
    }

    // Inline CSS from stylesheets
    let inlinedStyles = "";
    for (const sheet of document.styleSheets) {
      try {
        const rules = Array.from(sheet.cssRules || []);
        for (const rule of rules) {
          inlinedStyles += rule.cssText + "\n";
        }
      } catch {
        // Cross-origin stylesheet - try to fetch it
        if (sheet.href) {
          try {
            const cssText = await fetchText(sheet.href);
            if (cssText) {
              inlinedStyles += cssText + "\n";
            }
          } catch {
            // Skip inaccessible stylesheets
          }
        }
      }
    }

    // Remove existing link[rel=stylesheet] and add our inlined styles
    clone.querySelectorAll('link[rel="stylesheet"], link[rel="preload"]').forEach((el) => el.remove());

    // Inline background images in CSS
    inlinedStyles = await inlineCssUrls(inlinedStyles);

    const styleEl = clone.ownerDocument.createElement("style");
    styleEl.textContent = inlinedStyles;
    const head = clone.querySelector("head");
    if (head) {
      head.appendChild(styleEl);
    }

    // Add base tag to resolve any remaining relative URLs
    const existingBase = clone.querySelector("base");
    if (!existingBase) {
      const base = clone.ownerDocument.createElement("base");
      base.href = window.location.href;
      if (head) {
        head.prepend(base);
      }
    }

    // Wait for image conversions (with timeout)
    await Promise.race([
      Promise.allSettled(imagePromises),
      new Promise((resolve) => setTimeout(resolve, 8000)),
    ]);

    // Build final HTML
    const doctype = document.doctype
      ? `<!DOCTYPE ${document.doctype.name}>`
      : "<!DOCTYPE html>";

    return doctype + "\n" + clone.outerHTML;
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
  }

  // --- Utility functions ---

  async function fetchAsDataUri(url) {
    try {
      const response = await fetch(url, { mode: "cors" });
      if (!response.ok) return null;
      const blob = await response.blob();
      if (blob.size > 10 * 1024 * 1024) return null; // Skip >10MB
      return new Promise((resolve) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result);
        reader.onerror = () => resolve(null);
        reader.readAsDataURL(blob);
      });
    } catch {
      return null;
    }
  }

  async function fetchText(url) {
    try {
      const response = await fetch(url, { mode: "cors" });
      if (!response.ok) return null;
      return await response.text();
    } catch {
      return null;
    }
  }

  async function inlineCssUrls(css) {
    // Find url() references in CSS and inline small ones
    const urlRegex = /url\(["']?((?!data:)[^"')]+)["']?\)/g;
    const matches = [...css.matchAll(urlRegex)];

    for (const match of matches) {
      try {
        const url = new URL(match[1], window.location.href).href;
        const dataUri = await fetchAsDataUri(url);
        if (dataUri) {
          css = css.replace(match[0], `url("${dataUri}")`);
        }
      } catch {
        // Keep original URL
      }
    }

    return css;
  }
})();
