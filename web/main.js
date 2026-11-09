// Page logic (issues #17, #19, #21): file choice, two equal actions (image optimization and page
// PDF), progress, download, items to check. Display only: all decisions and processing happen in
// Rust, inside worker.js; the human text of plans and reports comes from Rust in Korean.
// Data from the zip (file names, paths) is inserted with textContent only, never as HTML.

const $ = (id) => document.getElementById(id);

const DEFAULTS = { max_width: 1920, max_height: 1080, max_kb: 1024, quality: 90, min_quality: 50 };
// Keep reasons (codes from the report) as short phrases; same wording as the Korean report.
const REASONS = {
  within_limits: "기준 이내",
  not_resized: "변환 대상 형식 아님",
  not_supported_by_app: "ez-CGER 미지원 형식",
  undecodable: "읽을 수 없음",
  too_many_pixels: "해상도가 너무 큼",
  encode_failed: "저장 실패",
  no_gain: "다시 저장해도 줄지 않음",
};
const CHECKS_SHOWN = 5;

let worker = null;
let currentFile = null;
let currentPlan = null;
let downloadUrl = null;
let pdfQueue = [];

// ---- small DOM helpers (textContent only) ----
function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else node.setAttribute(k, v);
  }
  for (const child of children.flat(Infinity)) {
    if (child == null) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

function size(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1 << 20) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1 << 20)).toFixed(1)} MB`;
}

function imageInfo(info) {
  const dims = info.width != null ? `${info.width}×${info.height} ` : "";
  const quality = info.quality != null ? ` 화질 ${info.quality}` : "";
  return `${dims}${info.format.toUpperCase()}${quality} · ${size(info.bytes)}`;
}

// "1, 20, 38페이지", "배경", or the first few and a count; same as the Korean report.
function pages(usedOn) {
  const shown = 8;
  const background = usedOn.some((u) => u.page == null);
  const numbers = [...new Set(usedOn.filter((u) => u.page != null).map((u) => u.page))].sort((a, b) => a - b);
  const parts = [];
  if (background) parts.push("배경");
  if (numbers.length) {
    const more = numbers.length > shown ? ` 등 ${numbers.length}개 페이지` : "";
    parts.push(`${numbers.slice(0, shown).join(", ")}페이지${more}`);
  }
  return parts.join(", ");
}

function showError(message) {
  const box = $("error");
  box.textContent = message;
  box.hidden = false;
}

function clearError() {
  $("error").hidden = true;
}

function setStatus(text) {
  $("status").textContent = text;
}

function showOptState(state) {
  for (const id of ["opt-ready", "opt-running", "opt-done"]) $(id).hidden = id !== `opt-${state}`;
}

// ---- settings ----
function settingsJson() {
  const num = (id) => Number($(id).value);
  return JSON.stringify({
    max_width: num("max-width"),
    max_height: num("max-height"),
    max_bytes: Math.round(num("max-kb") * 1024),
    jpeg_quality: num("quality"),
    min_jpeg_quality: num("min-quality"),
    language: "ko",
  });
}

function resetSettings() {
  $("max-width").value = DEFAULTS.max_width;
  $("max-height").value = DEFAULTS.max_height;
  $("max-kb").value = DEFAULTS.max_kb;
  $("quality").value = DEFAULTS.quality;
  $("min-quality").value = DEFAULTS.min_quality;
}

// ---- worker ----
function startWorker() {
  worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  worker.onmessage = (event) => handle(event.data);
  worker.onerror = (event) => {
    showError(`처리 중 오류가 발생했습니다. 페이지를 새로 고친 뒤 다시 시도하십시오. (${event.message || "worker stopped"})`);
    $("work").hidden = true;
    setStatus("");
  };
  worker.postMessage({ type: "version" });
}

async function open(file) {
  clearError();
  $("work").hidden = true;
  if (!/\.(zip|hcg)$/i.test(file.name)) {
    showError("zip 파일(hcg 파일과 이미지 폴더를 함께 압축한 파일) 또는 hcg 파일을 선택하십시오.");
    return;
  }
  currentFile = file;
  $("drop-text").textContent = `${file.name} (${size(file.size)})`;
  setStatus("파일을 읽는 중입니다…");
  if (!worker) startWorker();
  const buffer = await file.arrayBuffer();
  worker.postMessage({ type: "open", buffer, name: file.name, settings: settingsJson() }, [buffer]);
}

function handle(msg) {
  if (msg.type === "version") {
    $("ver").textContent = msg.version;
  } else if (msg.type === "plan") {
    setStatus("");
    renderPlan(msg.plan);
  } else if (msg.type === "progress") {
    $("bar").max = Math.max(msg.total, 1);
    $("bar").value = msg.done;
    $("progress-text").textContent = msg.name ? `${msg.done + 1} / ${msg.total} · ${msg.name}` : "결과 파일을 만드는 중입니다…";
  } else if (msg.type === "done") {
    renderResult(msg.zip, msg.reports);
  } else if (msg.type === "overview") {
    saveOverview(msg.index, msg.pdf);
  } else if (msg.type === "overview-error") {
    pdfQueue = [];
    $("pdf").disabled = false;
    $("pdf-note").textContent = "";
    showError(msg.message);
  } else if (msg.type === "error") {
    setStatus("");
    showOptState("ready");
    showError(msg.message);
  }
}

// ---- plan: summary line, two cards, items to check ----
function renderPlan(plan) {
  currentPlan = plan;
  const readable = plan.bundles.filter((b) => b.hcg);
  const pagesTotal = readable.reduce((n, b) => n + b.hcg.pages, 0);

  const what = plan.bundles.length === 1
    ? `${plan.bundles[0].bundle.stem}${readable.length ? ` — ${pagesTotal}페이지` : ""}`
    : `hcg 파일 ${plan.bundles.length}개${readable.length ? ` — ${pagesTotal}페이지` : ""}`;
  $("summary").textContent = plan.bundles.length ? `${what} · ${plan.summary.headline}` : plan.summary.headline;

  renderOptimizeCard(plan);
  renderPdfCard(readable);
  renderChecks(plan.bundles.flatMap((b) => checkItems(b)));
  showOptState("ready");
  $("details").hidden = true;
  $("details").open = false;
  $("work").hidden = false;
}

function renderOptimizeCard(plan) {
  const s = plan.summary;
  const maxBytes = Math.round(Number($("max-kb").value) * 1024);
  const note = $("opt-note");
  note.hidden = true;
  if (s.files_to_shrink > 0) {
    $("opt-summary").textContent =
      `대상 ${s.files_to_shrink}개 · ${size(s.bytes_to_shrink)} → 최대 ${size(s.files_to_shrink * maxBytes)}`;
  } else {
    $("opt-summary").textContent = s.files_total ? `이미지 ${s.files_total}개 확인` : "";
    note.hidden = false;
    const noFolder = plan.bundles.some((b) => b.hcg && b.bundle.folder_path == null && b.files.length === 0);
    note.textContent = s.nothing_to_do
      ? "hcg 파일을 읽을 수 없어 최적화할 수 없습니다."
      : noFolder
        ? "축소할 이미지가 없습니다. 이미지 폴더가 함께 들어 있지 않습니다."
        : "축소할 이미지가 없습니다. 모든 이미지가 기준 이내입니다.";
  }
  $("optimize").disabled = s.files_to_shrink === 0;

  const files = plan.bundles.flatMap((b) => b.files);
  $("opt-details").hidden = files.length === 0;
  const rows = files.map((f) =>
    el("tr", {},
      el("td", { class: "file" }, f.file),
      el("td", {}, imageInfo(f.before)),
      el("td", {}, f.action === "shrink"
        ? [el("span", { class: "tag shrink" }, "축소"), ` ${f.target.width}×${f.target.height}로`]
        : [el("span", { class: "tag keep" }, "유지"), ` ${REASONS[f.reason] || f.reason}`]),
      el("td", { class: "muted" }, pages(f.used_on))));
  $("opt-list").replaceChildren(el("div", { class: "table-wrap" },
    el("table", {}, el("thead", {}, el("tr", {}, el("th", {}, "파일"), el("th", {}, "현재"),
      el("th", {}, "처리"), el("th", {}, "사용 위치"))), el("tbody", {}, ...rows))));
}

function renderPdfCard(readable) {
  const perSheet = Number($("per-sheet").value);
  const pagesTotal = readable.reduce((n, b) => n + b.hcg.pages, 0);
  const sheets = readable.reduce((n, b) => n + Math.max(1, Math.ceil(b.hcg.pages / perSheet)), 0);
  $("pdf-summary").textContent = readable.length
    ? `${pagesTotal}페이지 · A4 ${sheets}장${readable.length > 1 ? ` · PDF ${readable.length}개` : ""}`
    : "hcg 파일을 읽을 수 없어 PDF를 만들 수 없습니다.";
  $("pdf").disabled = readable.length === 0;
  $("pdf-note").textContent = "";
}

// Items to check for one bundle (plan or report): failure with its advice, then warnings. A
// missing image folder gets one action line instead of repeating it per file.
function checkItems(b) {
  const items = [];
  if (b.failure) items.push({ text: `${b.bundle.hcg_path}: ${b.failure.message}`, action: b.failure.advice });
  const warnings = b.warnings.filter((w) => !b.failure || w.code !== b.failure.code);
  if (warnings.some((w) => w.code === "missing_image")) {
    items.push({ action: "hcg 파일과 이미지 폴더를 함께 압축하여 다시 선택하십시오.", strong: true });
  }
  for (const w of warnings) items.push({ text: w.message });
  return items;
}

function renderChecks(items) {
  const list = $("check-list");
  list.replaceChildren();
  const li = (item) => el("li", item.strong ? { class: "action" } : {},
    item.text || "", item.text && item.action ? el("br") : null,
    item.action ? (item.text ? `조치: ${item.action}` : item.action) : null);
  const first = items.slice(0, CHECKS_SHOWN + 1);
  for (const item of first) list.append(li(item));
  const rest = items.slice(CHECKS_SHOWN + 1);
  if (rest.length) {
    list.append(el("li", { class: "more" },
      el("details", {}, el("summary", {}, `나머지 ${rest.length}건 보기`), el("ul", {}, ...rest.map(li)))));
  }
  $("checks").hidden = items.length === 0;
}

// ---- page PDF ----
function requestPdfs() {
  clearError();
  const indexes = currentPlan.bundles.map((b, i) => (b.hcg ? i : -1)).filter((i) => i >= 0);
  if (!indexes.length) return;
  pdfQueue = indexes;
  $("pdf").disabled = true;
  $("pdf-note").textContent = "PDF를 만드는 중입니다…";
  nextPdf();
}

function nextPdf() {
  const index = pdfQueue.shift();
  if (index === undefined) return;
  worker.postMessage({ type: "overview", index, settings: JSON.stringify({ per_sheet: Number($("per-sheet").value) }) });
}

function saveOverview(index, pdfBuffer) {
  const name = `${currentPlan.bundles[index].bundle.stem}_pages.pdf`;
  const url = URL.createObjectURL(new Blob([pdfBuffer], { type: "application/pdf" }));
  const link = el("a", { href: url, download: name });
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60000);
  $("pdf-note").textContent = `저장됨: ${name} (${size(pdfBuffer.byteLength)})`;
  if (pdfQueue.length) nextPdf();
  else $("pdf").disabled = false;
}

// ---- result ----
function renderResult(zipBuffer, reports) {
  if (downloadUrl) URL.revokeObjectURL(downloadUrl);
  const blob = new Blob([zipBuffer], { type: "application/zip" });
  downloadUrl = URL.createObjectURL(blob);
  const stem = currentFile.name.replace(/\.(zip|hcg)$/i, "");
  const link = $("download");
  link.href = downloadUrl;
  link.download = `${stem}_optimized.zip`;
  link.textContent = `결과 파일 다운로드 (${size(blob.size)})`;

  const failed = reports.some((r) => r.status !== "ok");
  const lines = reports.map((r) => (reports.length > 1 ? `${r.bundle.stem}: ` : "") + r.summary.headline);
  $("result-headline").textContent = `${failed ? "일부 처리 실패" : "최적화 완료"} — ${lines.join(" / ")}`;

  const box = $("reports");
  box.replaceChildren();
  for (const r of reports) {
    const section = el("div", { class: "bundle" }, el("h3", {}, r.bundle.hcg_path));
    if (r.images.length) {
      const rows = r.images.map((i) => {
        const after = i.after ? imageInfo(i.after) : `유지 (${REASONS[i.reason] || "처리하지 않음"})`;
        const note = i.renamed_to ? `파일 이름 변경: ${i.renamed_to.file}` : "";
        return el("tr", {}, el("td", { class: "file" }, i.file), el("td", {}, imageInfo(i.before)),
          el("td", {}, after), el("td", { class: "muted" }, note));
      });
      section.append(el("div", { class: "table-wrap" },
        el("table", {}, el("thead", {}, el("tr", {}, el("th", {}, "파일"), el("th", {}, "이전"),
          el("th", {}, "이후"), el("th", {}, "비고"))), el("tbody", {}, ...rows))));
    }
    section.append(el("pre", { class: "text" }, r.text));
    box.append(section);
  }
  renderChecks(reports.flatMap((r) => checkItems(r)));
  $("details").hidden = false;
  showOptState("done");
}

// ---- events ----
$("file").addEventListener("change", (e) => {
  if (e.target.files[0]) open(e.target.files[0]);
  e.target.value = "";
});
const drop = $("drop");
drop.addEventListener("dragover", (e) => { e.preventDefault(); drop.classList.add("over"); });
drop.addEventListener("dragleave", () => drop.classList.remove("over"));
drop.addEventListener("drop", (e) => {
  e.preventDefault();
  drop.classList.remove("over");
  if (e.dataTransfer.files[0]) open(e.dataTransfer.files[0]);
});
drop.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") $("file").click(); });

$("optimize").addEventListener("click", () => {
  clearError();
  $("bar").value = 0;
  $("progress-text").textContent = "시작하는 중입니다…";
  showOptState("running");
  worker.postMessage({ type: "optimize" });
});
$("cancel").addEventListener("click", () => {
  // Terminating the worker stops the work at once; read the same file again.
  worker.terminate();
  worker = null;
  setStatus("취소했습니다.");
  if (currentFile) open(currentFile);
});
$("pdf").addEventListener("click", requestPdfs);
$("per-sheet").addEventListener("change", () => {
  if (currentPlan) renderPdfCard(currentPlan.bundles.filter((b) => b.hcg));
});
const optimizing = () => !$("opt-running").hidden || !$("opt-done").hidden;
for (const id of ["max-width", "max-height", "max-kb", "quality", "min-quality"]) {
  $(id).addEventListener("change", () => { if (currentFile && !optimizing()) open(currentFile); });
}
$("reset").addEventListener("click", () => {
  resetSettings();
  if (currentFile && !optimizing()) open(currentFile);
});

startWorker();
