// Old MikuMikuDance motions (.vmd from MMD 3.x, signature "Vocaloid Motion Data file")
// use a 10-byte model name; babylon-mmd only reads the newer "Vocaloid Motion Data 0002"
// files with a 20-byte name. Everything after the name is laid out the same way,
// so an old file is upgraded by rewriting its header.

const SIGNATURE_BYTES = 30;
const NEW_SIGNATURE = "Vocaloid Motion Data 0002";
const OLD_SIGNATURE = "Vocaloid Motion Data file";

function signature(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < SIGNATURE_BYTES && bytes[i] !== 0; i++) s += String.fromCharCode(bytes[i]);
  return s;
}

/** Whether `buffer` is an old-format VMD that needs upgrading. */
export function isOldVmd(buffer: ArrayBuffer): boolean {
  return buffer.byteLength >= SIGNATURE_BYTES + 10 && signature(new Uint8Array(buffer)) === OLD_SIGNATURE;
}

const BONE_FRAME = 111;
const MORPH_FRAME = 23;

/**
 * The same motion in the newer VMD format (new-format files come back unchanged).
 * Only the bone and morph keyframes are kept: old files lay out their camera
 * keyframes differently (36 bytes instead of 61), and read as new ones they turn
 * into nonsense frames far past the end of the dance.
 */
export function upgradeVmd(buffer: ArrayBuffer): ArrayBuffer {
  if (!isOldVmd(buffer)) return buffer;
  const src = new Uint8Array(buffer);
  const view = new DataView(buffer);
  const headerEnd = SIGNATURE_BYTES + 10;
  let end = headerEnd;
  const section = (frameBytes: number) => {
    if (end + 4 > src.length) return false;
    const count = view.getUint32(end, true);
    const next = end + 4 + count * frameBytes;
    if (next > src.length) return false;
    end = next;
    return true;
  };
  if (!section(BONE_FRAME)) return buffer; // not readable as a motion: let the loader say so
  section(MORPH_FRAME); // old files may stop right after the bones
  const body = src.subarray(headerEnd, end);
  const hasMorphs = end > headerEnd + 4 + view.getUint32(headerEnd, true) * BONE_FRAME;
  // New layout: signature, 20-byte name, bones, morphs, then empty camera, light,
  // self-shadow and IK sections.
  const out = new Uint8Array(SIGNATURE_BYTES + 20 + body.length + (hasMorphs ? 0 : 4) + 4 * 4);
  for (let i = 0; i < NEW_SIGNATURE.length; i++) out[i] = NEW_SIGNATURE.charCodeAt(i);
  out.set(src.subarray(SIGNATURE_BYTES, headerEnd), SIGNATURE_BYTES); // model name, zero-padded to 20
  out.set(body, SIGNATURE_BYTES + 20);
  return out.buffer; // the counts after the body are already zero
}
