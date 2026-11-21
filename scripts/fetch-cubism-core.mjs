// Downloads the Live2D Cubism Core runtime into public/vendor/.
// Cubism Core is Live2D proprietary "Redistributable Code", so it is fetched
// from Live2D at install time instead of being committed to this repo.
// Override with CUBISM_CORE_FILE=/path/to/live2dcubismcore.min.js (offline installs, tests).
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = resolve(root, "public/vendor/live2dcubismcore.min.js");
const url = "https://cubism.live2d.com/sdk-web/cubismcore/live2dcubismcore.min.js";

mkdirSync(dirname(out), { recursive: true });

if (process.env.CUBISM_CORE_FILE) {
  copyFileSync(process.env.CUBISM_CORE_FILE, out);
  console.log(`[cubism-core] copied from ${process.env.CUBISM_CORE_FILE}`);
} else if (existsSync(out) && !process.env.CUBISM_CORE_REFRESH) {
  console.log("[cubism-core] already present");
} else {
  try {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    writeFileSync(out, Buffer.from(await res.arrayBuffer()));
    console.log(`[cubism-core] downloaded from ${url}`);
  } catch (err) {
    console.warn(`[cubism-core] could not download ${url}: ${err.message}`);
    console.warn("[cubism-core] download it by hand into public/vendor/, or set CUBISM_CORE_FILE.");
    if (process.env.CI) process.exit(1);
  }
}
