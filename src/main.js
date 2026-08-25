import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  decisionPreview,
  formatBytes,
  selectedCandidateIds,
  suggestedCandidateIds,
} from "./presentation.js";
import "./styles.css";

const MIN_SCANNING_INDICATOR_MS = 600;
const MIN_SAVE_INDICATOR_MS = 600;

const state = {
  scan: null,
  selected: new Set(),
  filter: "all",
  busy: false,
  lastOutputPath: null,
  statusTimer: null,
  activeSaveJobId: null,
};

const app = document.querySelector("#app");
app.innerHTML = `
  <main class="shell">
    <header id="introHeader" class="hero">
      <div>
        <div class="eyebrow">OFFLINE DOCUMENT REPAIR</div>
        <h1>DOCX Break Cleaner</h1>
        <p>Find paragraph marks hidden inside Japanese OCR text, review them, and save a separate cleaned copy.</p>
      </div>
    </header>

    <section id="dropZone" class="drop-zone" aria-label="Choose or drop a DOCX file">
      <div class="drop-icon">¶</div>
      <div>
        <h2>Drop a DOCX here</h2>
        <p>or choose a file from this computer</p>
      </div>
      <button id="chooseButton" class="button button-primary" type="button">Choose DOCX</button>
    </section>

    <div id="status" class="status" role="status" aria-live="polite" hidden></div>

    <div id="saveModal" class="modal-backdrop" hidden>
      <section class="save-modal" role="dialog" aria-modal="true" aria-labelledby="saveModalTitle">
        <div class="label">SAVING DOCUMENT</div>
        <h2 id="saveModalTitle">Saving cleaned copy</h2>
        <p id="saveProgressMessage">Preparing cleaned copy…</p>
        <div id="saveProgressTrack" class="progress-track" role="progressbar" aria-label="Save progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow="0">
          <div id="saveProgressBar" class="progress-bar"></div>
        </div>
        <div class="save-modal-footer">
          <span id="saveProgressPercent">0%</span>
          <button id="cancelSaveButton" class="button button-quiet" type="button">Cancel</button>
        </div>
      </section>
    </div>

    <section id="workspace" class="workspace" hidden>
      <div class="workspace-top">
        <div class="document-bar">
          <div>
            <div class="label">DOCUMENT</div>
            <strong id="fileName"></strong>
            <div id="filePath" class="muted path"></div>
          </div>
          <button id="rescanButton" class="button button-quiet" type="button">Rescan</button>
        </div>

        <button id="loadedFilePicker" class="loaded-drop-zone" type="button">
          <span class="loaded-drop-icon">¶</span>
          <span><strong>Open another DOCX</strong><small>Drop here or click to choose</small></span>
        </button>
      </div>

      <div class="repair-bar">
        <div><strong id="selectedCount">0 lines set to merge</strong><span>The source file will not be changed.</span></div>
        <button id="saveButton" class="button button-primary" type="button">Save cleaned copy</button>
      </div>

      <div class="toolbar">
        <div class="tabs" role="tablist" aria-label="Candidate filter">
          <button class="tab active" data-filter="all" type="button">All (<span id="allCount">0</span>)</button>
          <button class="tab" data-filter="certain" type="button">Certain (<span id="certainCount">0</span>)</button>
          <button class="tab" data-filter="review" type="button">Review (<span id="reviewCount">0</span>)</button>
        </div>
      </div>

      <div id="candidateList" class="candidate-list"></div>
    </section>
  </main>
`;

const elements = {
  shell: document.querySelector(".shell"),
  introHeader: document.querySelector("#introHeader"),
  chooseButton: document.querySelector("#chooseButton"),
  loadedFilePicker: document.querySelector("#loadedFilePicker"),
  dropZone: document.querySelector("#dropZone"),
  status: document.querySelector("#status"),
  workspace: document.querySelector("#workspace"),
  fileName: document.querySelector("#fileName"),
  filePath: document.querySelector("#filePath"),
  allCount: document.querySelector("#allCount"),
  certainCount: document.querySelector("#certainCount"),
  reviewCount: document.querySelector("#reviewCount"),
  candidateList: document.querySelector("#candidateList"),
  selectedCount: document.querySelector("#selectedCount"),
  saveButton: document.querySelector("#saveButton"),
  rescanButton: document.querySelector("#rescanButton"),
  saveModal: document.querySelector("#saveModal"),
  saveProgressMessage: document.querySelector("#saveProgressMessage"),
  saveProgressTrack: document.querySelector("#saveProgressTrack"),
  saveProgressBar: document.querySelector("#saveProgressBar"),
  saveProgressPercent: document.querySelector("#saveProgressPercent"),
  cancelSaveButton: document.querySelector("#cancelSaveButton"),
};

elements.chooseButton.addEventListener("click", chooseDocument);
elements.loadedFilePicker.addEventListener("click", chooseDocument);
elements.rescanButton.addEventListener("click", () => state.scan && scanDocument(state.scan.inputPath));
elements.saveButton.addEventListener("click", saveCleanedCopy);
elements.cancelSaveButton.addEventListener("click", cancelActiveSave);

for (const tab of document.querySelectorAll(".tab")) {
  tab.addEventListener("click", () => {
    setCandidateFilter(tab.dataset.filter);
  });
}

function setCandidateFilter(filter) {
  state.filter = filter;
  document.querySelectorAll(".tab").forEach((tab) => {
    tab.classList.toggle("active", tab.dataset.filter === filter);
  });
  renderCandidates();
}

if (window.__TAURI_INTERNALS__) {
  getCurrentWebview()
    .onDragDropEvent((event) => {
      const payload = event.payload;
      const activeDropZone = elements.workspace.hidden
        ? elements.dropZone
        : elements.loadedFilePicker;
      activeDropZone.classList.toggle("dragging", payload.type === "over");
      if (payload.type !== "drop") return;
      const path = payload.paths?.find((value) => value.toLowerCase().endsWith(".docx"));
      if (path) scanDocument(path);
      else showStatus("Drop one DOCX file.", "error");
    })
    .catch((error) => showStatus(String(error), "error"));
}

async function chooseDocument() {
  const path = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Word document", extensions: ["docx"] }],
  });
  if (typeof path === "string") await scanDocument(path);
}

async function scanDocument(path) {
  if (state.busy) return;
  const scanStartedAt = window.performance.now();
  setBusy(true);
  showStatus("Scanning", "working");
  await waitForPaint();
  try {
    const result = await invoke("scan_docx", { path });
    await waitForMinimumScanTime(scanStartedAt);
    state.scan = result;
    state.selected = new Set(suggestedCandidateIds(result.candidates));
    state.lastOutputPath = null;
    elements.shell.classList.add("document-loaded");
    elements.introHeader.hidden = true;
    elements.dropZone.hidden = true;
    elements.workspace.hidden = false;
    elements.fileName.textContent = result.fileName;
    elements.filePath.textContent = result.inputPath;
    elements.allCount.textContent = result.candidates.length;
    elements.certainCount.textContent = result.certainCount;
    elements.reviewCount.textContent = result.reviewCount;
    setCandidateFilter("review");
    const complexNote = result.excludedComplexBoundaries
      ? ` ${result.excludedComplexBoundaries} complex boundaries were excluded for safety.`
      : "";
    showStatus(
      `Scan complete. ${result.candidates.length} candidate breaks found.${complexNote}`,
      "success",
      3000,
    );
  } catch (error) {
    await waitForMinimumScanTime(scanStartedAt);
    showStatus(String(error), "error");
  } finally {
    setBusy(false);
  }
}

async function waitForMinimumScanTime(startedAt) {
  const remaining = MIN_SCANNING_INDICATOR_MS - (window.performance.now() - startedAt);
  if (remaining > 0) {
    await new Promise((resolve) => window.setTimeout(resolve, remaining));
  }
}

function waitForPaint() {
  return new Promise((resolve) => {
    window.requestAnimationFrame(() => window.requestAnimationFrame(resolve));
  });
}

function renderCandidates() {
  const candidates = (state.scan?.candidates ?? []).filter(
    (candidate) => state.filter === "all" || candidate.confidence === state.filter,
  );
  elements.candidateList.replaceChildren();
  if (!candidates.length) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = state.scan?.candidates.length
      ? "No findings in this filter."
      : "No suspicious hidden paragraph breaks were found.";
    elements.candidateList.append(empty);
  }
  for (const candidate of candidates) {
    const card = document.createElement("article");
    card.className = `candidate ${candidate.confidence}`;

    const content = document.createElement("div");
    content.className = "candidate-content";
    const top = document.createElement("div");
    top.className = "candidate-top";
    const meta = document.createElement("div");
    meta.className = "candidate-meta";
    const badge = document.createElement("span");
    badge.className = "confidence-badge";
    badge.textContent = candidate.confidence === "certain" ? "CERTAIN" : "REVIEW";
    const location = document.createElement("span");
    location.className = "candidate-location";
    location.textContent = `Paragraphs ${candidate.firstParagraph}–${candidate.secondParagraph}`;
    meta.append(badge, location);

    const decision = document.createElement("div");
    decision.className = "decision-control";
    decision.setAttribute("role", "radiogroup");
    decision.setAttribute(
      "aria-label",
      `Decision for paragraphs ${candidate.firstParagraph} and ${candidate.secondParagraph}`,
    );
    const mergeButton = document.createElement("button");
    mergeButton.type = "button";
    mergeButton.className = "decision-option";
    mergeButton.setAttribute("role", "radio");
    mergeButton.textContent = "Merge";
    const keepButton = document.createElement("button");
    keepButton.type = "button";
    keepButton.className = "decision-option";
    keepButton.setAttribute("role", "radio");
    keepButton.textContent = "Don’t merge";
    decision.append(mergeButton, keepButton);
    top.append(decision, meta);

    const preview = document.createElement("div");
    preview.className = "decision-preview";
    const reason = document.createElement("p");
    reason.className = "candidate-reason";
    reason.textContent = candidate.reason;

    function applyDecision(shouldMerge) {
      if (shouldMerge) state.selected.add(candidate.id);
      else state.selected.delete(candidate.id);
      mergeButton.classList.toggle("selected", shouldMerge);
      keepButton.classList.toggle("selected", !shouldMerge);
      mergeButton.setAttribute("aria-checked", String(shouldMerge));
      keepButton.setAttribute("aria-checked", String(!shouldMerge));
      preview.classList.toggle("merged", shouldMerge);
      preview.classList.toggle("unmerged", !shouldMerge);
      preview.textContent = decisionPreview(candidate, shouldMerge);
      updateSelectedCount();
    }

    mergeButton.addEventListener("click", () => applyDecision(true));
    keepButton.addEventListener("click", () => applyDecision(false));
    content.append(top, preview, reason);
    card.append(content);
    elements.candidateList.append(card);
    applyDecision(state.selected.has(candidate.id));
  }
  updateSelectedCount();
}

function updateSelectedCount() {
  const count = selectedCandidateIds(state.scan?.candidates ?? [], state.selected).length;
  elements.selectedCount.textContent = `${count} ${count === 1 ? "line" : "lines"} set to merge`;
  elements.saveButton.disabled = state.busy || count === 0;
}

async function saveCleanedCopy() {
  if (!state.scan || state.busy) return;
  const candidateIds = selectedCandidateIds(state.scan.candidates, state.selected);
  if (!candidateIds.length) return;
  const outputPath = await save({
    defaultPath: state.scan.defaultOutputPath,
    filters: [{ name: "Word document", extensions: ["docx"] }],
  });
  if (typeof outputPath !== "string") return;
  const jobId = crypto.randomUUID();
  state.activeSaveJobId = jobId;
  setBusy(true);
  const saveStartedAt = window.performance.now();
  clearStatusTimer();
  elements.status.hidden = true;
  showSaveModal();
  let unlisten = () => {};
  try {
    unlisten = await listen("repair-progress", ({ payload }) => {
      if (payload.jobId === jobId) updateSaveProgress(payload.percent, payload.message);
    });
    const result = await invoke("repair_docx", {
      inputPath: state.scan.inputPath,
      outputPath,
      candidateIds,
      jobId,
    });
    state.lastOutputPath = result.outputPath;
    updateSaveProgress(100, "Cleaned copy saved.");
    await waitForMinimumSaveTime(saveStartedAt);
    hideSaveModal();
    showSuccessWithOpen(result);
  } catch (error) {
    await waitForMinimumSaveTime(saveStartedAt);
    hideSaveModal();
    const message = String(error);
    showStatus(message, message.includes("Save cancelled") ? "notice" : "error", 3000);
  } finally {
    unlisten();
    state.activeSaveJobId = null;
    setBusy(false);
  }
}

async function waitForMinimumSaveTime(startedAt) {
  const remaining = MIN_SAVE_INDICATOR_MS - (window.performance.now() - startedAt);
  if (remaining > 0) {
    await new Promise((resolve) => window.setTimeout(resolve, remaining));
  }
}

function showSaveModal() {
  updateSaveProgress(0, "Preparing cleaned copy…");
  elements.cancelSaveButton.disabled = false;
  elements.cancelSaveButton.textContent = "Cancel";
  elements.saveModal.hidden = false;
  elements.cancelSaveButton.focus();
}

function hideSaveModal() {
  elements.saveModal.hidden = true;
}

function updateSaveProgress(percent, message) {
  const safePercent = Math.max(0, Math.min(100, Number(percent) || 0));
  elements.saveProgressMessage.textContent = message;
  elements.saveProgressBar.style.width = `${safePercent}%`;
  elements.saveProgressPercent.textContent = `${safePercent}%`;
  elements.saveProgressTrack.setAttribute("aria-valuenow", String(safePercent));
}

async function cancelActiveSave() {
  if (!state.activeSaveJobId || elements.cancelSaveButton.disabled) return;
  elements.cancelSaveButton.disabled = true;
  elements.cancelSaveButton.textContent = "Cancelling…";
  elements.saveProgressMessage.textContent = "Cancelling save…";
  try {
    await invoke("cancel_repair", { jobId: state.activeSaveJobId });
  } catch (error) {
    elements.cancelSaveButton.disabled = false;
    elements.cancelSaveButton.textContent = "Cancel";
    showStatus(String(error), "error");
  }
}

function showSuccessWithOpen(result) {
  clearStatusTimer();
  elements.status.replaceChildren();
  elements.status.hidden = false;
  elements.status.className = "status success";
  const text = document.createElement("span");
  text.textContent = `Cleaned copy saved with ${result.mergedCount} repairs (${formatBytes(result.outputBytes)}).`;
  const button = document.createElement("button");
  button.className = "text-button";
  button.type = "button";
  button.textContent = "Open cleaned copy";
  button.addEventListener("click", () => openPath(result.outputPath));
  elements.status.append(text, button);
  state.statusTimer = window.setTimeout(() => {
    elements.status.hidden = true;
    state.statusTimer = null;
  }, 5000);
}

function clearStatusTimer() {
  if (state.statusTimer !== null) {
    window.clearTimeout(state.statusTimer);
    state.statusTimer = null;
  }
}

function showStatus(message, kind, autoDismissMs = 0) {
  clearStatusTimer();
  elements.status.replaceChildren();
  elements.status.hidden = false;
  elements.status.className = `status ${kind}`;
  elements.status.textContent = message;
  if (autoDismissMs > 0) {
    state.statusTimer = window.setTimeout(() => {
      elements.status.hidden = true;
      state.statusTimer = null;
    }, autoDismissMs);
  }
}

function setBusy(value) {
  state.busy = value;
  elements.chooseButton.disabled = value;
  elements.loadedFilePicker.disabled = value;
  elements.rescanButton.disabled = value;
  updateSelectedCount();
}

if (import.meta.env.DEV && new URLSearchParams(location.search).has("demo")) {
  state.scan = {
    inputPath: "C:\\Documents\\The Great Rebellion 偉大なる反乱.docx",
    fileName: "The Great Rebellion 偉大なる反乱.docx",
    certainCount: 42,
    reviewCount: 24,
    candidates: [
      {
        id: "p562-p563",
        firstParagraph: 562,
        secondParagraph: 563,
        confidence: "certain",
        suggestedMerge: true,
        reason: "A verified Japanese word or inflected form is split across two Word paragraphs.",
        beforeText: "幼稚園、小学校、中学校、高校、大学などでプログラ",
        afterText: "ムされたロボットであることは明らかだ。",
        joinedText: "幼稚園、小学校、中学校、高校、大学などでプログラムされたロボットであることは明らかだ。",
      },
      {
        id: "p593-p594",
        firstParagraph: 593,
        secondParagraph: 594,
        confidence: "review",
        suggestedMerge: true,
        reason: "Paragraph formatting and incomplete text make this look like a hidden continuation.",
        beforeText: "私たちが持つ人間的人格について何も知らない",
        afterText: "というのは皮肉なものだ。",
        joinedText: "私たちが持つ人間的人格について何も知らないというのは皮肉なものだ。",
      },
    ],
  };
  state.selected = new Set(suggestedCandidateIds(state.scan.candidates));
  elements.shell.classList.add("document-loaded");
  elements.introHeader.hidden = true;
  elements.dropZone.hidden = true;
  elements.workspace.hidden = false;
  elements.fileName.textContent = state.scan.fileName;
  elements.filePath.textContent = state.scan.inputPath;
  elements.allCount.textContent = state.scan.candidates.length;
  elements.certainCount.textContent = state.scan.certainCount;
  elements.reviewCount.textContent = state.scan.reviewCount;
  setCandidateFilter("review");
  showStatus(
    "Scan complete. 66 candidate breaks found. 41 complex boundaries were excluded for safety.",
    "success",
    3000,
  );
}
