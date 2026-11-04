// Page logic (issue #17): file choice, plan preview, progress, download, report viewer.
// Display only: all decisions and processing happen in Rust, inside worker.js.
// Data from the zip (file names, paths) is inserted with textContent only, never as HTML.

const $ = (id) => document.getElementById(id);

const DEFAULTS = { max_width: 1920, max_height: 1080, max_kb: 1024, quality: 90, min_quality: 50 };
const REASONS = {
  within_limits: "already within limits",
  not_resized: "format left as is",
  not_supported_by_app: "format not supported by ez-CGER",
  undecodable: "could not be read",
  too_many_pixels: "too large to process",
  encode_failed: "could not be saved",
  no_gain: "re-saving would not make it smaller",
};

let worker = null;
let currentFile = null;
let downloadUrl = null;

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
  const quality = info.quality != null ? ` q${info.quality}` : "";
  return `${dims}${info.format.toUpperCase()}${quality} · ${size(info.bytes)}`;
}

function places(usedOn) {
  return usedOn
    .map((u) => (u.page == null ? "background" : `page ${u.page}`) + (u.frame ? ` (frame ${u.frame})` : ""))
    .filter((v, i, a) => a.indexOf(v) === i)
    .join(", ");
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

function show(section) {
  for (const id of ["plan", "progress", "result"]) $(id).hidden = id !== section;
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
    showError(`Something went wrong: ${event.message || "the processing worker stopped"}.`);
    show(null);
    setStatus("");
  };
  worker.postMessage({ type: "version" });
}

async function open(file) {
  clearError();
  show(null);
  if (!/\.zip$/i.test(file.name)) {
    showError("Please choose a .zip file containing the .hcg file and its image folder.");
    return;
  }
  currentFile = file;
  $("drop-text").textContent = `${file.name} (${size(file.size)})`;
  setStatus("Reading the zip…");
  if (!worker) startWorker();
  const buffer = await file.arrayBuffer();
  worker.postMessage({ type: "open", buffer, settings: settingsJson() }, [buffer]);
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
    $("progress-text").textContent = msg.name ? `${msg.done + 1} / ${msg.total} · ${msg.name}` : "Writing the zip…";
  } else if (msg.type === "done") {
    renderResult(msg.zip, msg.reports);
  } else if (msg.type === "error") {
    setStatus("");
    show(null);
    showError(msg.message);
  }
}

// ---- plan ----
function renderPlan(plan) {
  $("plan-headline").textContent = plan.summary.headline;
  const body = $("plan-body");
  body.replaceChildren();
  for (const b of plan.bundles) {
    const box = el("div", { class: "bundle" }, el("h3", {}, b.bundle.hcg_path));
    if (b.failure) {
      box.append(el("p", {}, el("span", { class: "tag failed" }, "cannot be processed"), " ", b.failure.message));
      box.append(el("p", { class: "muted" }, b.failure.advice));
    }
    if (b.files.length) {
      const rows = b.files.map((f) =>
        el("tr", {},
          el("td", { class: "file" }, f.file),
          el("td", {}, imageInfo(f.before)),
          el("td", {}, f.action === "shrink"
            ? [el("span", { class: "tag shrink" }, "shrink"), ` to fit ${f.target.width}×${f.target.height}`]
            : [el("span", { class: "tag keep" }, "keep"), ` ${REASONS[f.reason] || f.reason}`]),
          el("td", { class: "muted" }, places(f.used_on))));
      box.append(el("div", { class: "table-wrap" },
        el("table", {}, el("thead", {}, el("tr", {}, el("th", {}, "File"), el("th", {}, "Now"),
          el("th", {}, "Plan"), el("th", {}, "Used on"))), el("tbody", {}, ...rows))));
    }
    if (b.warnings.length) {
      box.append(el("ul", { class: "warnings" }, ...b.warnings.map((w) => el("li", {}, w.message))));
    }
    body.append(box);
  }
  for (const name of plan.other_files.unpaired) {
    body.append(el("p", { class: "muted" }, `Not part of any .hcg (left out): ${name}`));
  }
  $("optimize").disabled = plan.summary.nothing_to_do;
  show("plan");
}

// ---- result ----
function renderResult(zipBuffer, reports) {
  if (downloadUrl) URL.revokeObjectURL(downloadUrl);
  const blob = new Blob([zipBuffer], { type: "application/zip" });
  downloadUrl = URL.createObjectURL(blob);
  const stem = currentFile.name.replace(/\.zip$/i, "");
  const link = $("download");
  link.href = downloadUrl;
  link.download = `${stem}_optimized.zip`;
  link.textContent = `Download ${stem}_optimized.zip (${size(blob.size)})`;

  const box = $("reports");
  box.replaceChildren();
  for (const r of reports) {
    const section = el("div", { class: "bundle" },
      el("h3", {}, r.bundle.hcg_path, " ",
        el("span", { class: r.status === "ok" ? "tag ok" : "tag failed" }, r.status === "ok" ? "optimized" : "failed")),
      el("p", { class: "headline" }, r.summary.headline));
    if (r.failure) section.append(el("p", { class: "muted" }, r.failure.advice));
    if (r.warnings.length) {
      section.append(el("ul", { class: "warnings" }, ...r.warnings.map((w) => el("li", {}, w.message))));
    }
    if (r.images.length) {
      const rows = r.images.map((i) => {
        const after = i.after ? imageInfo(i.after) : `kept (${REASONS[i.reason] || i.reason || "not processed"})`;
        const note = i.renamed_to ? `renamed to ${i.renamed_to.file}` : "";
        return el("tr", {}, el("td", { class: "file" }, i.file), el("td", {}, imageInfo(i.before)),
          el("td", {}, after), el("td", { class: "muted" }, note));
      });
      section.append(el("div", { class: "table-wrap" },
        el("table", {}, el("thead", {}, el("tr", {}, el("th", {}, "File"), el("th", {}, "Before"),
          el("th", {}, "After"), el("th", {}, ""))), el("tbody", {}, ...rows))));
    }
    section.append(el("details", {}, el("summary", {}, "Full report (report.txt)"), el("pre", { class: "text" }, r.text)));
    box.append(section);
  }
  setStatus("");
  show("result");
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
  $("progress-text").textContent = "Starting…";
  show("progress");
  worker.postMessage({ type: "optimize" });
});
$("cancel").addEventListener("click", () => {
  // Terminating the worker stops the work at once; re-plan the same file.
  worker.terminate();
  worker = null;
  setStatus("Cancelled.");
  if (currentFile) open(currentFile);
});
for (const id of ["max-width", "max-height", "max-kb", "quality", "min-quality"]) {
  $(id).addEventListener("change", () => { if (currentFile && $("result").hidden) open(currentFile); });
}
$("reset").addEventListener("click", () => {
  resetSettings();
  if (currentFile && $("result").hidden) open(currentFile);
});

startWorker();
