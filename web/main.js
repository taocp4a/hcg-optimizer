// Loads the WASM module (dist/pkg, produced by build_web.sh) and wires it to the page.
import init, { version } from "./pkg/hcg_wasm.js";

await init();
document.getElementById("ver").textContent = version();
