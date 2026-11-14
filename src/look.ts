// Which model parameters follow the mouse.
//
// pixi-live2d-display drives the Cubism 3+ standard IDs (ParamAngleX,
// ParamEyeBallX, ...). Many models, Aqua among them, were converted from
// Cubism 2 and still use PARAM_ANGLE_X-style IDs, so they never looked at the
// cursor. This finds the matching parameters by name in whatever style the
// model uses, and settings.json can override them per model.

export const LOOK_ROLES = ["angle_x", "angle_y", "angle_z", "eye_x", "eye_y", "body_x", "breath"] as const;
export type LookRole = (typeof LOOK_ROLES)[number];
export type LookParams = Record<LookRole, string | null>;

/** Lowercase letters and digits only: PARAM_ANGLE_X, ParamAngleX and param-angle-x all become "paramanglex". */
export function normalize(id: string): string {
  return id.toLowerCase().replace(/[^a-z0-9]/g, "");
}

// Normalised names for each role, best first.
const CANDIDATES: Record<LookRole, string[]> = {
  angle_x: ["paramanglex", "anglex", "paramheadanglex", "headanglex", "paramheadx", "headx", "paramfacex", "faceanglex"],
  angle_y: ["paramangley", "angley", "paramheadangley", "headangley", "paramheady", "heady", "paramfacey", "faceangley"],
  angle_z: ["paramanglez", "anglez", "paramheadanglez", "headanglez", "paramheadz", "headz", "paramfacez", "faceanglez"],
  eye_x: ["parameyeballx", "eyeballx", "parameyex", "parameyesx", "eyex", "parampupilx", "pupilx"],
  eye_y: ["parameyebally", "eyebally", "parameyey", "parameyesy", "eyey", "parampupily", "pupily"],
  body_x: ["parambodyanglex", "bodyanglex", "parambodyx", "bodyx", "paramupperbodyanglex"],
  breath: ["parambreath", "breath"],
};

/**
 * Picks a parameter for each role from the model's parameter IDs.
 * `override` (from settings) wins; an empty string there turns a role off.
 */
export function detectLookParams(ids: string[], override: Partial<Record<string, string>> = {}): LookParams {
  const byName = new Map<string, string>();
  for (const id of ids) {
    const n = normalize(id);
    if (!byName.has(n)) byName.set(n, id);
  }
  const out = {} as LookParams;
  for (const role of LOOK_ROLES) {
    const forced = override[role];
    if (typeof forced === "string") {
      out[role] = forced === "" ? null : forced;
      continue;
    }
    out[role] = null;
    for (const c of CANDIDATES[role]) {
      const id = byName.get(c);
      if (id) {
        out[role] = id;
        break;
      }
    }
  }
  return out;
}
