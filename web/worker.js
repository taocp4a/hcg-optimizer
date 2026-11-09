// Web Worker (issue #17): owns the WASM session so the page never freezes. Messages:
//   in:  {type:"version"} | {type:"open", buffer, name, settings} | {type:"optimize"}
//        | {type:"overview", index, settings}
//   out: {type:"version", version} | {type:"plan", plan} | {type:"progress", done, total, name}
//        | {type:"done", zip, reports} | {type:"overview", index, pdf}
//        | {type:"error", message} | {type:"overview-error", message}
import init, { Session, version } from "./pkg/hcg_wasm.js";

const ready = init();
let session = null;

function describe(err) {
  const text = String(err && err.message ? err.message : err);
  // A wasm trap or failed allocation usually means the tab ran out of memory.
  if (err instanceof WebAssembly.RuntimeError || /memory|unreachable/i.test(text)) {
    return "The browser ran out of memory while processing this zip. Close other tabs and try again, " +
      "or split the project into smaller zips.";
  }
  return text;
}

self.onmessage = async (event) => {
  const msg = event.data;
  try {
    await ready;
    if (msg.type === "version") {
      self.postMessage({ type: "version", version: version() });
    } else if (msg.type === "open") {
      if (session) session.free();
      session = null;
      session = new Session(new Uint8Array(msg.buffer), msg.name, msg.settings);
      self.postMessage({ type: "plan", plan: JSON.parse(session.planJson()) });
    } else if (msg.type === "optimize") {
      const zip = session.optimize((done, total, name) => {
        self.postMessage({ type: "progress", done, total, name });
      });
      const reports = JSON.parse(session.reportsJson());
      self.postMessage({ type: "done", zip: zip.buffer, reports }, [zip.buffer]);
    } else if (msg.type === "overview") {
      const pdf = session.overviewPdf(msg.index, msg.settings);
      self.postMessage({ type: "overview", index: msg.index, pdf: pdf.buffer }, [pdf.buffer]);
    }
  } catch (err) {
    // An overview failure leaves the plan usable; anything else ends the current step.
    const type = msg.type === "overview" ? "overview-error" : "error";
    self.postMessage({ type, message: describe(err) });
  }
};
