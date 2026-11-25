// Calmer physics for MMD models' hair, chest and clothes.
//
// Models are rigged for MikuMikuDance, which steps physics at 120 Hz or more; the
// raw values let hair, skirts and ribbons swing far and stretch away from the body
// once she moves quickly (a dance). Before the model's physics is built, this makes
// every simulated part heavier-feeling (more damping), lets joints swing less, and
// almost removes the sliding that joints allow, which is what reads as stretching.
// Kept free of Babylon so it can be unit tested.

type Vec3 = [number, number, number];

/** The rigid body fields this touches (PMX rigid body). */
export interface BodyLike {
  name: string;
  linearDamping: number;
  angularDamping: number;
  /** 0: follows its bone (not simulated), 1 and 2: simulated. */
  physicsMode: number;
}

/** The joint fields this touches (PMX joint). */
export interface JointLike {
  rigidbodyIndexA: number;
  rigidbodyIndexB: number;
  positionMin: Vec3;
  positionMax: Vec3;
  rotationMin: Vec3;
  rotationMax: Vec3;
  springRotation: Vec3;
}

export interface PhysicsTuning {
  /** How far damping moves towards 1 (fully damped): 0 keeps the model's own. */
  damping: number;
  /** Scale for how far joints may swing. */
  swing: number;
  /** Scale for how far joints may slide (stretch). */
  stretch: number;
  /** Scale for the springs pulling joints back to rest (1 keeps the model's own). */
  spring: number;
  /** Lowest spring strength, for joints the model left without one. */
  minSpring: number;
}

/** Heavier, steadier clothes and hair: Roger found the model's own values far too floaty. */
export const STEADY: PhysicsTuning = { damping: 0.5, swing: 0.7, stretch: 0.2, spring: 1, minSpring: 0 };

/** The chest, stiffer still (Roger asked twice for a stiffer chest). */
export const STEADY_CHEST: PhysicsTuning = { damping: 0.9, swing: 0.15, stretch: 0, spring: 4, minSpring: 200 };

/** Rigid body names used for the chest (Japanese, Chinese, English). */
const CHEST = /胸|乳|おっぱい|オッパイ|bust|breast|boob|oppai|chest|mune/i;

export function isChestBody(name: string): boolean {
  return CHEST.test(name);
}

const MAX_DAMPING = 0.995;

function towardsOne(d: number, k: number): number {
  const v = Number.isFinite(d) ? Math.min(1, Math.max(0, d)) : 0;
  return Math.min(MAX_DAMPING, v + (1 - v) * k);
}

const scale = (v: Vec3, k: number): Vec3 => [v[0] * k, v[1] * k, v[2] * k];
const stiffen = (v: Vec3, t: PhysicsTuning): Vec3 =>
  t.spring === 1 && t.minSpring === 0 ? v : [0, 1, 2].map((i) => Math.max(v[i] * t.spring, t.minSpring)) as Vec3;

/** Copies of the model's rigid bodies and joints with `tuning` applied. */
export function steadyPhysics<B extends BodyLike, J extends JointLike>(
  bodies: readonly B[],
  joints: readonly J[],
  tuning: PhysicsTuning = STEADY,
  chest: PhysicsTuning = STEADY_CHEST,
): { bodies: B[]; joints: J[] } {
  const chestBody = bodies.map((b) => b.physicsMode !== 0 && isChestBody(b.name));
  return {
    bodies: bodies.map((b, i) => {
      if (b.physicsMode === 0) return b;
      const t = chestBody[i] ? chest : tuning;
      return {
        ...b,
        linearDamping: towardsOne(b.linearDamping, t.damping),
        angularDamping: towardsOne(b.angularDamping, t.damping),
      };
    }),
    joints: joints.map((j) => {
      // A joint holding a simulated chest body uses the chest tuning.
      const t = chestBody[j.rigidbodyIndexA] || chestBody[j.rigidbodyIndexB] ? chest : tuning;
      return {
        ...j,
        positionMin: scale(j.positionMin, t.stretch),
        positionMax: scale(j.positionMax, t.stretch),
        rotationMin: scale(j.rotationMin, t.swing),
        rotationMax: scale(j.rotationMax, t.swing),
        springRotation: stiffen(j.springRotation, t),
      };
    }),
  };
}
