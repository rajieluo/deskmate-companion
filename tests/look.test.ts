import { describe, expect, it } from "vitest";
import { detectLookParams } from "../src/look";

describe("detectLookParams", () => {
  it("finds Cubism 3 standard IDs", () => {
    const p = detectLookParams(["ParamAngleX", "ParamAngleY", "ParamAngleZ", "ParamEyeBallX", "ParamEyeBallY", "ParamBodyAngleX", "ParamBreath"]);
    expect(p).toEqual({
      angle_x: "ParamAngleX", angle_y: "ParamAngleY", angle_z: "ParamAngleZ",
      eye_x: "ParamEyeBallX", eye_y: "ParamEyeBallY", body_x: "ParamBodyAngleX", breath: "ParamBreath",
    });
  });

  it("finds Cubism 2 style IDs like Aqua's", () => {
    const ids = ["PARAM_ANGLE_X", "PARAM_ANGLE_Y", "PARAM_ANGLE_Z", "PARAM_BODY_ANGLE_X", "PARAM_BROW_L_ANGLE",
      "PARAM_EYE_BALL_X", "PARAM_EYE_BALL_Y", "PARAM_EYE_BALL_FORM", "PARAM_JISUANJI_ANGLE_X"];
    const p = detectLookParams(ids);
    expect(p.angle_x).toBe("PARAM_ANGLE_X");
    expect(p.eye_x).toBe("PARAM_EYE_BALL_X");
    expect(p.eye_y).toBe("PARAM_EYE_BALL_Y");
    expect(p.body_x).toBe("PARAM_BODY_ANGLE_X");
    expect(p.breath).toBeNull();
  });

  it("leaves unknown names alone and applies overrides", () => {
    const ids = ["Param", "Param2", "Param12", "Param13"];
    expect(detectLookParams(ids).angle_x).toBeNull();
    const p = detectLookParams(ids, { angle_x: "Param12", eye_x: "Param13", body_x: "" });
    expect(p.angle_x).toBe("Param12");
    expect(p.eye_x).toBe("Param13");
    expect(p.body_x).toBeNull();
  });

  it("an override of \"\" turns a detected role off", () => {
    expect(detectLookParams(["ParamBodyAngleX"], { body_x: "" }).body_x).toBeNull();
  });
});
