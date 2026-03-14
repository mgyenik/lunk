// Popup script: save/queue UI for the current tab.

const $ = (sel) => document.querySelector(sel);

let currentTab = null;
let isSaving = false;

// --- Init ---

document.addEventListener("DOMContentLoaded", async () => {
  // Get current tab
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  currentTab = tab;

  // Show page title
  $("#pageTitle").textContent = tab.title || "Untitled";

  // Check connection to backend
  checkConnection();

  // Check if page is already saved
  checkPageStatus(tab.url);

  // Button handlers
  $("#btnSave").addEventListener("click", () => savePage("read"));
  $("#btnQueue").addEventListener("click", () => savePage("unread"));
});

async function checkConnection() {
  try {
    const response = await chrome.runtime.sendMessage({
      target: "background",
      action: "ping",
    });

    if (response?.success) {
      $("#connectionDot").classList.add("connected");
      $("#connectionDot").title = "Connected to Lunk";
    } else {
      $("#connectionDot").classList.add("disconnected");
      $("#connectionDot").title = "Not connected to Lunk";
    }
  } catch {
    $("#connectionDot").classList.add("disconnected");
    $("#connectionDot").title = "Not connected to Lunk";
  }
}

async function checkPageStatus(url) {
  if (!url || url.startsWith("chrome://") || url.startsWith("chrome-extension://")) {
    $("#statusBar").textContent = "Cannot save this page";
    return;
  }

  try {
    const response = await chrome.runtime.sendMessage({
      target: "background",
      action: "check_status",
      url: url,
    });

    if (response?.success && response.data?.saved) {
      const entry = response.data.entry;
      $("#statusBar").className = "status-bar saved";
      $("#statusBar").textContent = `Already saved (${entry.status})`;
      $("#btnSave").textContent = "Save Again";
    } else {
      $("#statusBar").textContent = "Not saved yet";
    }

    // Enable buttons
    $("#btnSave").disabled = false;
    $("#btnQueue").disabled = false;
  } catch {
    $("#statusBar").textContent = "Ready to save";
    $("#btnSave").disabled = false;
    $("#btnQueue").disabled = false;
  }
}

async function savePage(status) {
  if (isSaving || !currentTab?.id) return;
  isSaving = true;

  // Disable buttons, show progress
  $("#btnSave").disabled = true;
  $("#btnQueue").disabled = true;
  $("#actions").style.display = "none";
  $("#progress").classList.add("active");
  $("#result").classList.remove("active");

  try {
    // Step 1: Extract content
    setProgress(20, "Extracting readable content...");

    setProgress(50, "Capturing page snapshot...");

    // Send save request to background
    const response = await chrome.runtime.sendMessage({
      target: "background",
      action: "save",
      tabId: currentTab.id,
      status: status,
    });

    setProgress(90, "Saving...");

    if (response?.success) {
      setProgress(100, "Done!");

      const entry = response.data;
      showResult(
        "success",
        `Saved: ${entry?.title || "page"}`
      );
    } else {
      showResult("error", `Failed: ${response?.error || "Unknown error"}`);
    }
  } catch (err) {
    showResult("error", `Error: ${err.message}`);
  } finally {
    isSaving = false;

    // Show buttons again after a delay
    setTimeout(() => {
      $("#progress").classList.remove("active");
      $("#actions").style.display = "flex";
      $("#btnSave").disabled = false;
      $("#btnQueue").disabled = false;
    }, 2000);
  }
}

function setProgress(percent, text) {
  $("#progressFill").style.width = `${percent}%`;
  $("#progressText").textContent = text;
}

function showResult(type, message) {
  const el = $("#result");
  el.className = `result active ${type}`;
  el.textContent = message;
}
