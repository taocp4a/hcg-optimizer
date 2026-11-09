// Browser end-to-end test of the web page (issue #17), run by web_e2e.sh.
//   node web_e2e.js <site-url> <input.zip> <out-dir> <bare.hcg>
// Checks (Korean page, issue #21): summary and two action cards, page PDF before and after
// optimizing and from a bare .hcg, cancel, optimize, download, Korean report, and no request
// leaves the site (W1).
const { chromium } = require("playwright");
const path = require("path");

const [url, input, outDir, bareHcg] = process.argv.slice(2);
const origin = new URL(url).origin;

function check(cond, message) {
  if (!cond) throw new Error(`FAIL: ${message}`);
  console.log(`ok: ${message}`);
}

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ acceptDownloads: true, viewport: { width: 1100, height: 900 } });
  const foreign = [];
  const errors = [];
  page.on("request", (r) => { if (!r.url().startsWith(origin) && !r.url().startsWith("blob:") && !r.url().startsWith("data:")) foreign.push(r.url()); });
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });

  await page.goto(url);
  await page.waitForFunction(() => document.getElementById("ver").textContent !== "…");
  check(true, `page loads, core ${await page.textContent("#ver")}`);

  // File chosen: summary line, two equal cards, image list (issue #21).
  const pdfDownload = async (label) => {
    const [d] = await Promise.all([page.waitForEvent("download"), page.click("#pdf")]);
    const saved = path.join(outDir, d.suggestedFilename());
    await d.saveAs(saved);
    const head = require("fs").readFileSync(saved).subarray(0, 5).toString();
    check(/_pages\.pdf$/.test(saved) && head === "%PDF-", `${label}: ${path.basename(saved)}`);
  };
  await page.setInputFiles("#file", input);
  await page.waitForSelector("#work:not([hidden])", { timeout: 60000 });
  const summary = await page.textContent("#summary");
  check(/축소 대상 이미지 2개/.test(summary), `summary line: "${summary}"`);
  check(await page.isVisible("#opt-card") && await page.isVisible("#pdf-card"), "both action cards shown");
  check(await page.isEnabled("#optimize"), "optimize enabled");
  check((await page.locator("#opt-list tbody tr").count()) > 0, "image list present");
  check(!(await page.locator("#work").innerText()).includes("[object"), "no stray object text");
  await page.screenshot({ path: path.join(outDir, "plan.png"), fullPage: true });

  // Page PDF before optimizing (issue #19).
  await page.selectOption("#per-sheet", "6");
  check(/A4 1장/.test(await page.textContent("#pdf-summary")), "sheet count follows the select");
  await pdfDownload("page PDF before optimizing");

  // Cancel returns to the ready state of the same file.
  await page.click("#optimize");
  await page.waitForSelector("#opt-running:not([hidden])");
  await page.click("#cancel");
  await page.waitForSelector("#opt-ready:not([hidden])", { timeout: 60000 });
  check(await page.isVisible("#work"), "cancel returns to the cards");

  // Optimize and download.
  await page.click("#optimize");
  await page.waitForSelector("#opt-done:not([hidden])", { timeout: 180000 });
  const headline = await page.textContent("#result-headline");
  check(/^최적화 완료 — 이미지 2개 축소/.test(headline), `result shown: "${headline}"`);
  const [download] = await Promise.all([page.waitForEvent("download"), page.click("#download")]);
  const saved = path.join(outDir, download.suggestedFilename());
  await download.saveAs(saved);
  check(/_optimized\.zip$/.test(saved), `downloaded ${path.basename(saved)}`);
  await page.click("#details summary");
  check(/hcg 이미지 최적화 점검 결과/.test(await page.textContent("#reports")), "detailed report in Korean");
  await page.screenshot({ path: path.join(outDir, "result.png"), fullPage: true });

  // The PDF still works after optimizing (issue #21).
  await pdfDownload("page PDF after optimizing");

  // A bare .hcg: optimize explains why it is disabled, the PDF works, the missing folder is listed.
  await page.setInputFiles("#file", bareHcg);
  await page.waitForSelector("#opt-ready:not([hidden]) #opt-note:not([hidden])", { timeout: 60000 });
  check(!(await page.isEnabled("#optimize")), "bare .hcg: optimize disabled");
  check(/이미지 폴더가 함께 들어 있지 않습니다/.test(await page.textContent("#opt-note")), "bare .hcg: reason shown");
  check(/함께 압축하여 다시 선택하십시오/.test(await page.textContent("#check-list")), "bare .hcg: action first");
  await page.screenshot({ path: path.join(outDir, "bare_hcg.png"), fullPage: true });
  await pdfDownload("bare .hcg page PDF");

  // Wrong file type gives a message, not a blank page.
  await page.setInputFiles("#file", { name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("x") });
  check(/zip 파일/.test(await page.textContent("#error")), "wrong file type shows a message");

  check(foreign.length === 0, `no request leaves the site (${foreign.join(", ") || "none"})`);
  check(errors.length === 0, `no page errors (${errors.join(" | ") || "none"})`);
  console.log(`SAVED ${saved}`);
  await browser.close();
})().catch((e) => { console.error(e.message || e); process.exit(1); });
