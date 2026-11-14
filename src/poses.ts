// Manual poses and animations: every motion and expression a model has, for the
// picker in the chat bar. VTube Studio models often keep expressions (poses like
// "sitting position", "Love eye") as loose .exp3.json files the .model3.json
// doesn't list, so the folder's files are included too.
import { motionName, type ModelSettingsJson, type MotionRef } from "./motions";

export const EXTRA_GROUP = "Extra";

export interface MotionAction {
  label: string;
  group: string;
  index: number;
}

export interface ExpressionAction {
  label: string;
  file: string;
}

export interface ModelActions {
  motions: MotionAction[];
  expressions: ExpressionAction[];
}

type Expressions = { Name?: string; File: string }[];

const norm = (f: string) => f.replace(/\\/g, "/").replace(/^\.\//, "").toLowerCase();

export function expressionName(file: string): string {
  const base = file.split(/[\\/]/).pop() ?? file;
  return base.replace(/\.exp3\.json$/i, "");
}

/**
 * Adds motion files found in the folder but missing from the settings to an
 * "Extra" group (so they can be played and classified like the rest), and
 * returns the list of motions and expressions to offer.
 */
export function withFolderFiles(json: ModelSettingsJson, files: string[]): { json: ModelSettingsJson; actions: ModelActions } {
  const motions = json.FileReferences.Motions ?? {};
  const listed = new Set<string>();
  for (const list of Object.values(motions)) for (const m of list) listed.add(norm(m.File));
  const extra: MotionRef[] = files
    .filter((f) => /\.motion3\.json$/i.test(f) && !listed.has(norm(f)))
    .map((File) => ({ File }));
  const groups: Record<string, MotionRef[]> = { ...motions };
  if (extra.length) groups[EXTRA_GROUP] = [...(groups[EXTRA_GROUP] ?? []), ...extra];

  const exprs = (json.FileReferences.Expressions as Expressions | undefined) ?? [];
  const exprFiles = new Set(exprs.map((e) => norm(e.File)));
  const expressions: ExpressionAction[] = [
    ...exprs.map((e) => ({ label: e.Name || expressionName(e.File), file: e.File })),
    ...files
      .filter((f) => /\.exp3\.json$/i.test(f) && !exprFiles.has(norm(f)))
      .map((file) => ({ label: expressionName(file), file })),
  ];

  const motionActions: MotionAction[] = [];
  const seen = new Set<string>();
  for (const [group, list] of Object.entries(groups)) {
    list.forEach((m, index) => {
      const key = norm(m.File);
      if (seen.has(key)) return;
      seen.add(key);
      motionActions.push({ label: motionName(m.File).replace(/^\d+[_-]/, "").replace(/_/g, " "), group, index });
    });
  }

  return {
    json: { ...json, FileReferences: { ...json.FileReferences, Motions: groups } },
    actions: { motions: motionActions, expressions },
  };
}

/** One parameter change from an .exp3.json. */
export interface ExpressionParam {
  id: string;
  value: number;
  blend: "Add" | "Multiply" | "Overwrite";
}

export function parseExpression(data: unknown): ExpressionParam[] {
  const params = (data as { Parameters?: { Id?: string; Value?: number; Blend?: string }[] })?.Parameters ?? [];
  return params
    .filter((p) => typeof p.Id === "string" && typeof p.Value === "number")
    .map((p) => ({
      id: p.Id!,
      value: p.Value!,
      blend: p.Blend === "Multiply" ? "Multiply" : p.Blend === "Overwrite" ? "Overwrite" : "Add",
    }));
}

/** Applies an expression's change to a parameter's current value. */
export function blend(current: number, p: ExpressionParam): number {
  if (p.blend === "Multiply") return current * p.value;
  if (p.blend === "Overwrite") return p.value;
  return current + p.value;
}
