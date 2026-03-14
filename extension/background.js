// Background service worker: manages native messaging, context menus, and keyboard shortcuts.

const NATIVE_HOST = "com.lunk.app";
const API_BASE = "http://127.0.0.1:9723/api/v1";

let nativePort = null;
let pendingRequests = new Map(); // requestId -> {resolve, reject, timeout}
let requestCounter = 0;

// --- Native Messaging ---

function connectNativeHost() {
  if (nativePort) return nativePort;

  try {
    nativePort = chrome.runtime.connectNative(NATIVE_HOST);

    nativePort.onMessage.addListener((msg) => {
      // Route response to pending request
      if (msg._requestId && pendingRequests.has(msg._requestId)) {
        const { resolve, timeout } = pendingRequests.get(msg._requestId);
        clearTimeout(timeout);
        pendingRequests.delete(msg._requestId);
        resolve(msg);
      }
    });

    nativePort.onDisconnect.addListener(() => {
      const error = chrome.runtime.lastError;
      console.warn("Lunk: native host disconnected:", error?.message || "unknown");
      nativePort = null;

      // Reject all pending requests
      for (const [id, { reject, timeout }] of pendingRequests) {
        clearTimeout(timeout);
        reject(new Error("Native host disconnected"));
      }
      pendingRequests.clear();
    });

    return nativePort;
  } catch (err) {
    console.warn("Lunk: failed to connect native host:", err);
    nativePort = null;
    return null;
  }
}

function sendNativeMessage(action, data) {
  return new Promise((resolve, reject) => {
    const port = connectNativeHost();

    if (!port) {
      // Fallback to HTTP API
      return sendHttpMessage(action, data).then(resolve).catch(reject);
    }

    const requestId = ++requestCounter;
    const timeoutHandle = setTimeout(() => {
      pendingRequests.delete(requestId);
      // Fallback to HTTP on timeout
      sendHttpMessage(action, data).then(resolve).catch(reject);
    }, 5000);

    pendingRequests.set(requestId, { resolve, reject, timeout: timeoutHandle });

    try {
      port.postMessage({ action, data, _requestId: requestId });
    } catch (err) {
      clearTimeout(timeoutHandle);
      pendingRequests.delete(requestId);
      // Fallback to HTTP
      sendHttpMessage(action, data).then(resolve).catch(reject);
    }
  });
}

// --- HTTP API Fallback ---

async function sendHttpMessage(action, data) {
  try {
    switch (action) {
      case "save_entry": {
        const body = {
          url: data.url,
          title: data.title,
          content_type: data.content_type || "article",
          extracted_text: data.extracted_text || "",
          status: data.status || "unread",
          source: "extension",
        };

        if (data.snapshot_html) {
          body.snapshot_html = btoa(
            new TextEncoder().encode(data.snapshot_html).reduce(
              (acc, byte) => acc + String.fromCharCode(byte), ""
            )
          );
        }
        if (data.readable_html) {
          body.readable_html = btoa(
            new TextEncoder().encode(data.readable_html).reduce(
              (acc, byte) => acc + String.fromCharCode(byte), ""
            )
          );
        }
        if (data.pdf_base64) {
          body.pdf_base64 = data.pdf_base64;
        }

        const resp = await fetch(`${API_BASE}/entries`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        });

        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        const entry = await resp.json();
        return { success: true, data: entry };
      }

      case "get_status": {
        const resp = await fetch(
          `${API_BASE}/entries?q=&limit=1&offset=0`
        );
        // Simple check - search by URL
        const url = data?.url;
        if (!url) return { success: true, data: { saved: false } };

        const listResp = await fetch(`${API_BASE}/entries?limit=200`);
        if (!listResp.ok) throw new Error(`HTTP ${listResp.status}`);
        const list = await listResp.json();
        const found = list.entries.find((e) => e.url === url);
        return {
          success: true,
          data: found ? { saved: true, entry: found } : { saved: false },
        };
      }

      case "ping": {
        const resp = await fetch(`${API_BASE}/health`);
        return { success: resp.ok, data: { pong: true } };
      }

      default:
        return { success: false, error: `Unknown action: ${action}` };
    }
  } catch (err) {
    return { success: false, error: err.message };
  }
}

// --- Context Menus ---

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "lunk-save",
    title: "Save to Lunk",
    contexts: ["page", "link"],
  });

  chrome.contextMenus.create({
    id: "lunk-queue",
    title: "Queue in Lunk (read later)",
    contexts: ["page", "link"],
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (!tab?.id) return;

  const status = info.menuItemId === "lunk-queue" ? "unread" : "read";

  try {
    await savePage(tab.id, status);
  } catch (err) {
    console.error("Lunk: context menu save failed:", err);
  }
});

// --- Keyboard Shortcuts ---

chrome.commands.onCommand.addListener(async (command) => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.id) return;

  if (command === "save-page") {
    await savePage(tab.id, "read");
  } else if (command === "queue-page") {
    await savePage(tab.id, "unread");
  }
});

// --- Core Save Logic ---

async function savePage(tabId, status) {
  // Send extraction request to content script
  const result = await chrome.tabs.sendMessage(tabId, {
    action: "extract",
    options: {},
  });

  if (result?.error) {
    throw new Error(result.error);
  }

  // Send to native host or HTTP API
  const saveData = {
    url: result.url,
    title: result.title,
    content_type: result.content_type || "article",
    extracted_text: result.extracted_text || "",
    readable_html: result.readable_html || null,
    snapshot_html: result.snapshot_html || null,
    pdf_base64: result.pdf_base64 || null,
    status: status,
  };

  const response = await sendNativeMessage("save_entry", saveData);

  if (!response.success) {
    throw new Error(response.error || "Save failed");
  }

  // Show saved badge on the tab
  updateBadge(tabId, true);

  return response.data;
}

// --- Badge ---

function updateBadge(tabId, saved) {
  if (saved) {
    chrome.action.setBadgeText({ text: "\u2713", tabId });
    chrome.action.setBadgeBackgroundColor({ color: "#22c55e", tabId });
  } else {
    chrome.action.setBadgeText({ text: "", tabId });
  }
}

// Check badge state when tab is activated or updated
chrome.tabs.onActivated.addListener(async ({ tabId }) => {
  try {
    const tab = await chrome.tabs.get(tabId);
    if (tab.url) checkAndUpdateBadge(tabId, tab.url);
  } catch { /* ignore */ }
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (changeInfo.status === "complete" && tab.url) {
    checkAndUpdateBadge(tabId, tab.url);
  }
});

async function checkAndUpdateBadge(tabId, url) {
  if (!url || url.startsWith("chrome://") || url.startsWith("chrome-extension://")) {
    updateBadge(tabId, false);
    return;
  }

  try {
    const response = await sendNativeMessage("get_status", { url });
    updateBadge(tabId, response?.success && response?.data?.saved);
  } catch {
    updateBadge(tabId, false);
  }
}

// --- Message Handler (from popup) ---

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.target !== "background") return false;

  switch (msg.action) {
    case "save": {
      const tabId = msg.tabId;
      const status = msg.status || "unread";

      savePage(tabId, status)
        .then((data) => sendResponse({ success: true, data }))
        .catch((err) => sendResponse({ success: false, error: err.message }));
      return true; // async
    }

    case "check_status": {
      const url = msg.url;
      sendNativeMessage("get_status", { url })
        .then((resp) => sendResponse(resp))
        .catch((err) => sendResponse({ success: false, error: err.message }));
      return true;
    }

    case "ping": {
      sendNativeMessage("ping", {})
        .then((resp) => sendResponse(resp))
        .catch((err) => sendResponse({ success: false, error: err.message }));
      return true;
    }
  }
});
