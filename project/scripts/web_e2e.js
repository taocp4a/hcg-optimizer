// Browser end-to-end test of the web page (issue #17), run by web_e2e.sh.
//   node web_e2e.js <site-url> <input.zip> <out-dir> <bare.hcg>
// Checks: plan shown, page overview PDF (zip and bare .hcg), optimize, download, report viewer,
// cancel, and no request leaves the site (W1).
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

  // Plan.
  await page.setInputFiles("#file", input);
  await page.waitForSelector("#plan:not([hidden])", { timeout: 60000 });
  const planHeadline = await page.textContent("#plan-headline");
  check(/will be shrunk/.test(planHeadline), `plan shown: "${planHeadline}"`);
  check((await page.locator("#plan-body tbody tr").count()) > 0, "plan lists the images");
  check(!(await page.locator("#plan-body").innerText()).includes("[object"), "no stray object text in the plan");
  await page.screenshot({ path: path.join(outDir, "plan.png"), fullPage: true });

  // Page overview PDF (issue #19), without leaving the plan.
  await page.selectOption("#per-sheet", "6");
  const [pdfDownload] = await Promise.all([page.waitForEvent("download"), page.click("button.overview")]);
  const pdfPath = path.join(outDir, pdfDownload.suggestedFilename());
  await pdfDownload.saveAs(pdfPath);
  const pdfHead = require("fs").readFileSync(pdfPath).subarray(0, 5).toString();
  check(/_pages\.pdf$/.test(pdfPath) && pdfHead === "%PDF-", `page overview downloaded: ${path.basename(pdfPath)}`);
  check(await page.locator("#plan").isVisible(), "plan stays after the overview");

  // Cancel, then the same file is planned again.
  await page.click("#optimize");
  await page.waitForSelector("#progress:not([hidden])");
  await page.click("#cancel");
  await page.waitForSelector("#plan:not([hidden])", { timeout: 60000 });
  check(true, "cancel returns to the plan");

  // Optimize and download.
  await page.click("#optimize");
  await page.waitForSelector("#result:not([hidden])", { timeout: 180000 });
  const headline = await page.textContent("#reports .headline");
  check(/images shrunk/.test(headline), `report shown: "${headline}"`);
  const [download] = await Promise.all([page.waitForEvent("download"), page.click("#download")]);
  const saved = path.join(outDir, download.suggestedFilename());
  await download.saveAs(saved);
  check(/_optimized\.zip$/.test(saved), `downloaded ${path.basename(saved)}`);
  await page.screenshot({ path: path.join(outDir, "result.png"), fullPage: true });

  // A bare .hcg (no image folder) still gives a plan and a page overview.
  await page.setInputFiles("#file", bareHcg);
  await page.waitForSelector("#plan:not([hidden])", { timeout: 60000 });
  check(/not in the image folder/.test(await page.innerText("#plan-body")), "bare .hcg plan reports the missing images");
  const [hcgPdf] = await Promise.all([page.waitForEvent("download"), page.click("button.overview")]);
  check(/_pages\.pdf$/.test(hcgPdf.suggestedFilename()), `bare .hcg gives ${hcgPdf.suggestedFilename()}`);

  // Wrong file type gives a message, not a blank page.
  await page.setInputFiles("#file", { name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("x") });
  check(/choose a \.zip/i.test(await page.textContent("#error")), "non-zip input shows a message");

  check(foreign.length === 0, `no request leaves the site (${foreign.join(", ") || "none"})`);
  check(errors.length === 0, `no page errors (${errors.join(" | ") || "none"})`);
  console.log(`SAVED ${saved}`);
  await browser.close();
})().catch((e) => { console.error(e.message || e); process.exit(1); });
