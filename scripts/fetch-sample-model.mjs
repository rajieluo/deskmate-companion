// Downloads a Live2D free sample model (Hiyori, from Live2D's CubismWebSamples)
// into tests/fixtures/ for local testing. Not committed.
// Usage: npm run fixtures
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const base = "https://raw.githubusercontent.com/Live2D/CubismWebSamples/develop/Samples/Resources/Hiyori/";
const manifest = "Hiyori.model3.json";
const outDir = resolve(root, "tests/fixtures/Hiyori");

async function get(name) {
  const res = await fetch(base + name);
  if (!res.ok) throw new Error(`${name}: HTTP ${res.status}`);
  const buf = Buffer.from(await res.arrayBuffer());
  const dest = resolve(outDir, name);
  mkdirSync(dirname(dest), { recursive: true });
  writeFileSync(dest, buf);
  return buf;
}

const json = JSON.parse((await get(manifest)).toString("utf8"));
const refs = json.FileReferences;
const files = [refs.Moc, ...(refs.Textures ?? []), refs.Physics, refs.Pose, refs.DisplayInfo, refs.UserData]
  .concat(Object.values(refs.Motions ?? {}).flat().flatMap((m) => [m.File, m.Sound]))
  .concat((refs.Expressions ?? []).map((e) => e.File))
  .filter(Boolean);
for (const f of new Set(files)) await get(f);
console.log(`Sample model saved to ${outDir}`);
