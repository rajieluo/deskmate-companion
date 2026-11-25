import { describe, expect, it } from "vitest";
import { bodyKind, isChestBody, steadyPhysics } from "../src/mmd-physics";

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
    expect(bodies[1].linearDamping).toBeCloseTo(0.65);
    expect(bodies[1].angularDamping).toBeCloseTo(0.85);
    // The model's own data isn't changed.
    expect(hair.linearDamping).toBe(0.5);
  });

  it("never fully freezes a part and copes with bad values", () => {
    const { bodies } = steadyPhysics([{ name: "前髪", linearDamping: 1, angularDamping: NaN, physicsMode: 2 }], []);
    expect(bodies[0].linearDamping).toBeLessThan(1);
    expect(bodies[0].angularDamping).toBeCloseTo(0.85);
  });

  it("lets hair joints swing less and barely stretch", () => {
    const hair = (name: string) => ({ name, linearDamping: 0, angularDamping: 0, physicsMode: 1 });
    const { joints } = steadyPhysics([hair("後髪1"), hair("後髪2")], [joint()]);
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
    expect(bodies[1].linearDamping).toBeCloseTo(0.92);
    expect(bodies[2].linearDamping).toBeCloseTo(0.75);
    expect(joints[0].rotationMax[0]).toBeCloseTo(0.15);
    expect(joints[0].springRotation).toEqual([400, 200, 200]);
    expect(joints[1].springRotation).toEqual([150, 0, 75]);
    expect(joints[0].positionMax[0]).toBeCloseTo(0);
    expect(joints[1].rotationMax[0]).toBeCloseTo(0.6);
    expect(joints[1].positionMax[0]).toBeCloseTo(0.15);
  });

  it("makes clothing stiffer than hair", () => {
    const sim = (name: string) => ({ name, linearDamping: 0.5, angularDamping: 0.5, physicsMode: 1 });
    const { bodies, joints } = steadyPhysics([sim("前髪"), sim("スカート"), sim("リボン")], [joint(0, 0), joint(1, 2), joint(0, 1)]);
    expect(bodies[0].linearDamping).toBeCloseTo(0.65);
    expect(bodies[1].linearDamping).toBeCloseTo(0.75);
    expect(joints[0].rotationMax[0]).toBeCloseTo(0.7);
    expect(joints[1].rotationMax[0]).toBeCloseTo(0.6);
    // A joint between hair and clothing takes the clothing's (stiffer) tuning.
    expect(joints[2].rotationMax[0]).toBeCloseTo(0.6);
  });

  it("sorts bodies into chest, hair and clothing by name", () => {
    for (const n of ["髪", "前髪2", "後ろ髪", "もみあげ_L", "アホ毛", "Hair_back", "ponytail", "发尾", "リボン"]) expect(bodyKind(n), n).toBe("hair");
    for (const n of ["左胸上", "Breast_R"]) expect(bodyKind(n), n).toBe("chest");
    for (const n of ["スカート前", "尻尾", "tail"]) expect(bodyKind(n), n).toBe("clothing");
    for (const n of ["コート", "マント", "coat", "Cape_L"]) expect(bodyKind(n), n).toBe("heavy");
  });

  it("recognises chest bodies by name", () => {
    for (const n of ["胸", "右おっぱい", "乳揺れ", "Breast_L", "bust", "胸部", "mune1"]) expect(isChestBody(n), n).toBe(true);
    for (const n of ["髪", "スカート", "リボン", "hair"]) expect(isChestBody(n), n).toBe(false);
  });
});
