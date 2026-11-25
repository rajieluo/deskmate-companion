import { describe, expect, it } from "vitest";
import { STEADY, isChestBody, steadyPhysics } from "../src/mmd-physics";

const joint = (a = 0, b = 1) => ({
  name: "j",
  rigidbodyIndexA: a,
  rigidbodyIndexB: b,
  positionMin: [-1, -2, 0] as [number, number, number],
  positionMax: [1, 2, 0] as [number, number, number],
  rotationMin: [-1, -0.5, 0] as [number, number, number],
  rotationMax: [1, 0.5, 0] as [number, number, number],
  springRotation: [100, 0, 50] as [number, number, number],
});

describe("steadyPhysics", () => {
  it("damps simulated parts more and leaves bone-following ones alone", () => {
    const follow = { name: "head", linearDamping: 0.1, angularDamping: 0.1, physicsMode: 0 };
    const hair = { name: "hair", linearDamping: 0.5, angularDamping: 0.2, physicsMode: 1 };
    const { bodies } = steadyPhysics([follow, hair], []);
    expect(bodies[0]).toBe(follow);
    expect(bodies[1].name).toBe("hair");
    expect(bodies[1].linearDamping).toBeCloseTo(0.75);
    expect(bodies[1].angularDamping).toBeCloseTo(0.6);
    // The model's own data isn't changed.
    expect(hair.linearDamping).toBe(0.5);
  });

  it("never fully freezes a part and copes with bad values", () => {
    const { bodies } = steadyPhysics([{ name: "x", linearDamping: 1, angularDamping: NaN, physicsMode: 2 }], []);
    expect(bodies[0].linearDamping).toBeLessThan(1);
    expect(bodies[0].angularDamping).toBeCloseTo(STEADY.damping);
  });

  it("lets joints swing less and barely stretch", () => {
    const { joints } = steadyPhysics([], [joint()]);
    expect(joints[0].positionMax).toEqual([0.2, 0.4, 0]);
    expect(joints[0].positionMin[1]).toBeCloseTo(-0.4);
    expect(joints[0].rotationMax[0]).toBeCloseTo(0.7);
    expect(joints[0].rotationMin[1]).toBeCloseTo(-0.35);
    expect(joints[0].name).toBe("j");
  });

  it("makes the chest stiffer than the rest", () => {
    const body = { name: "上半身", linearDamping: 0, angularDamping: 0, physicsMode: 0 };
    const chest = { name: "左胸", linearDamping: 0.5, angularDamping: 0.5, physicsMode: 1 };
    const skirt = { name: "スカート", linearDamping: 0.5, angularDamping: 0.5, physicsMode: 1 };
    const { bodies, joints } = steadyPhysics([body, chest, skirt], [joint(0, 1), joint(0, 2)]);
    expect(bodies[1].linearDamping).toBeCloseTo(0.95);
    expect(bodies[2].linearDamping).toBeCloseTo(0.75);
    expect(joints[0].rotationMax[0]).toBeCloseTo(0.15);
    expect(joints[0].springRotation).toEqual([400, 200, 200]);
    expect(joints[1].springRotation).toEqual([100, 0, 50]);
    expect(joints[0].positionMax[0]).toBeCloseTo(0);
    expect(joints[1].rotationMax[0]).toBeCloseTo(0.7);
  });

  it("recognises chest bodies by name", () => {
    for (const n of ["胸", "右おっぱい", "乳揺れ", "Breast_L", "bust", "胸部", "mune1"]) expect(isChestBody(n), n).toBe(true);
    for (const n of ["髪", "スカート", "リボン", "hair"]) expect(isChestBody(n), n).toBe(false);
  });
});
