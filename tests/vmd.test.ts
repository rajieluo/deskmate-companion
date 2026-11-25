import { describe, expect, it } from "vitest";
import { isOldVmd, upgradeVmd } from "../src/vmd";

function vmd(sig: string, nameBytes: number, rest: number[]): ArrayBuffer {
  const out = new Uint8Array(30 + nameBytes + rest.length);
  for (let i = 0; i < sig.length; i++) out[i] = sig.charCodeAt(i);
  out[30] = 0x41; // model name "A"
  out.set(rest, 30 + nameBytes);
  return out.buffer;
}

describe("upgradeVmd", () => {
  it("rewrites an old header, keeps bone and morph frames and drops the camera", () => {
    const bone = new Array(111).fill(7);
    const morph = new Array(23).fill(8);
    const camera = new Array(36).fill(9);
    const u32 = (n: number) => [n, 0, 0, 0];
    const old = vmd("Vocaloid Motion Data file", 10, [...u32(1), ...bone, ...u32(1), ...morph, ...u32(1), ...camera, 1, 2, 3]);
    expect(isOldVmd(old)).toBe(true);
    const up = new Uint8Array(upgradeVmd(old));
    expect(String.fromCharCode(...up.subarray(0, 25))).toBe("Vocaloid Motion Data 0002");
    expect(up[25]).toBe(0);
    expect(up[30]).toBe(0x41);
    expect([...up.subarray(50, 54)]).toEqual(u32(1));
    expect(up[54]).toBe(7);
    expect([...up.subarray(165, 169)]).toEqual(u32(1));
    expect(up[169]).toBe(8);
    // Empty camera, light, shadow and IK sections.
    expect([...up.subarray(192)]).toEqual(new Array(16).fill(0));
  });

  it("adds an empty morph section to files that stop after the bones", () => {
    const old = vmd("Vocaloid Motion Data file", 10, [1, 0, 0, 0, ...new Array(111).fill(7)]);
    const up = new Uint8Array(upgradeVmd(old));
    expect(up.length).toBe(50 + 4 + 111 + 4 + 16);
    expect([...up.subarray(165)]).toEqual(new Array(20).fill(0));
  });

  it("leaves new files alone", () => {
    const fresh = vmd("Vocaloid Motion Data 0002", 20, [0, 0, 0, 0]);
    expect(isOldVmd(fresh)).toBe(false);
    expect(upgradeVmd(fresh)).toBe(fresh);
  });
});

