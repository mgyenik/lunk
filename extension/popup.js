// Popup script: save UI + tag management for the current tab.

const $ = (sel) => document.querySelector(sel);

let currentTab = null;
let isSaving = false;
let savedEntryId = null;
let appliedTags = [];

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
  $("#btnSave").addEventListener("click", () => savePage());

  // Tag input: Enter to add
  $("#tagInput").addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      const val = e.target.value.trim().toLowerCase();
      if (val && !appliedTags.includes(val)) {
        addTag(val);
      }
      e.target.value = "";
    }
  });
});

async function checkConnection() {
  try {
    const response = await chrome.runtime.sendMessage({
      target: "background",
      action: "ping",
    });

    if (response?.success) {
      $("#connectionDot").classList.add("connected");
      $("#connectionDot").title = "Connected to Grymoire";
    } else {
      $("#connectionDot").classList.add("disconnected");
      $("#connectionDot").title = "Not connected to Grymoire";
    }
  } catch {
    $("#connectionDot").classList.add("disconnected");
    $("#connectionDot").title = "Not connected to Grymoire";
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
      $("#statusBar").textContent = "Already saved";
      $("#btnSave").textContent = "Save Again";

      // Show existing tags
      if (entry.tags && entry.tags.length > 0) {
        savedEntryId = entry.id;
        appliedTags = [...entry.tags];
        renderAppliedTags();
        $("#tagSection").classList.add("active");
      }
    } else {
      $("#statusBar").textContent = "Not saved yet";
    }

    // Enable buttons
    $("#btnSave").disabled = false;
  } catch {
    $("#statusBar").textContent = "Ready to save";
    $("#btnSave").disabled = false;
  }
}

async function savePage() {
  if (isSaving || !currentTab?.id) return;
  isSaving = true;

  // Collect tags (include read-later if checked)
  const tags = [];
  if ($("#chkReadLater").checked) {
    tags.push("read-later");
  }

  // Disable buttons, show progress
  $("#btnSave").disabled = true;
  $("#actions").style.display = "none";
  $("#progress").classList.add("active");
  $("#result").classList.remove("active");
  $("#tagSection").classList.remove("active");

  try {
    // Step 1: Extract content
    setProgress(20, "Extracting readable content...");

    setProgress(50, "Capturing page snapshot...");

    // Send save request to background
    const response = await chrome.runtime.sendMessage({
      target: "background",
      action: "save",
      tabId: currentTab.id,
      tags: tags,
    });

    setProgress(90, "Saving...");

    if (response?.success) {
      setProgress(100, "Done!");

      const entry = response.data;
      savedEntryId = entry?.id;
      appliedTags = entry?.tags || [...tags];

      if (entry?.index_status === "failed") {
        showResult(
          "warning",
          `Saved "${entry?.title || "page"}" but text extraction failed.`
        );
      } else {
        showResult(
          "success",
          `Saved: ${entry?.title || "page"}`
        );
      }

      // Show tag section and load suggestions
      renderAppliedTags();
      $("#tagSection").classList.add("active");
      loadTagSuggestions();
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
    }, 2000);
  }
}

// --- Tag Management ---

async function loadTagSuggestions() {
  if (!savedEntryId) return;

  try {
    // Extract domain from current tab URL
    let domain = null;
    try {
      domain = new URL(currentTab.url).hostname;
    } catch { /* ignore */ }

    const response = await chrome.runtime.sendMessage({
      target: "background",
      action: "get_tag_suggestions",
      data: {
        domain: domain,
        title: currentTab.title || "",
      },
    });

    if (response?.success && response.data) {
      renderSuggestions(response.data);
    }
  } catch {
    // Suggestions are optional, don't show error
  }
}

function renderSuggestions(suggestions) {
  // Collect all suggestions, deduplicated, excluding already-applied tags
  const all = [];
  const seen = new Set(appliedTags);

  for (const tag of (suggestions.domain_tags || [])) {
    if (!seen.has(tag)) { all.push(tag); seen.add(tag); }
  }
  for (const tag of (suggestions.similar_tags || [])) {
    if (!seen.has(tag)) { all.push(tag); seen.add(tag); }
  }
  for (const tag of (suggestions.popular_tags || [])) {
    if (!seen.has(tag)) { all.push(tag); seen.add(tag); }
  }

  const container = $("#suggestedTags");
  container.innerHTML = "";

  if (all.length === 0) {
    $("#suggestedLabel").style.display = "none";
    return;
  }

  $("#suggestedLabel").style.display = "block";

  for (const tag of all) {
    const chip = document.createElement("span");
    chip.className = "tag-chip";
    chip.textContent = tag;
    chip.addEventListener("click", () => {
      addTag(tag);
    });
    container.appendChild(chip);
  }
}

function renderAppliedTags() {
  const container = $("#appliedTags");
  container.innerHTML = "";

  for (const tag of appliedTags) {
    const chip = document.createElement("span");
    chip.className = "tag-chip active";
    chip.innerHTML = `${tag} <span class="remove">&times;</span>`;
    chip.querySelector(".remove").addEventListener("click", (e) => {
      e.stopPropagation();
      removeTag(tag);
    });
    container.appendChild(chip);
  }
}

function addTag(tag) {
  if (appliedTags.includes(tag)) return;
  appliedTags.push(tag);
  renderAppliedTags();
  updateTags();

  // Remove from suggestions display
  const suggestedChips = $("#suggestedTags").querySelectorAll(".tag-chip");
  for (const chip of suggestedChips) {
    if (chip.textContent === tag) {
      chip.remove();
      break;
    }
  }
}

function removeTag(tag) {
  appliedTags = appliedTags.filter((t) => t !== tag);
  renderAppliedTags();
  updateTags();

  // Re-render suggestions to include the removed tag
  loadTagSuggestions();
}

async function updateTags() {
  if (!savedEntryId) return;

  try {
    await chrome.runtime.sendMessage({
      target: "background",
      action: "update_tags",
      data: {
        id: savedEntryId,
        tags: appliedTags,
      },
    });
  } catch {
    // Tag update failed silently — not critical
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
