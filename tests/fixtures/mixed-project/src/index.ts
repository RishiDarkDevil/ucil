/**
 * Mixed-project TypeScript component.
 *
 * This file intentionally contains several lint/type defects used to test
 * UCIL's diagnostic capabilities. Do not clean up these defects.
 */

// DEFECT 1: implicit `any` via loose function parameter type.
// In a strict-mode project this would require an explicit type annotation.
// Here the parameter `data` has no annotation, so it is implicitly `any`.
export function processData(data: any): string { // eslint-disable-line @typescript-eslint/no-explicit-any
  return String(data);
}

// DEFECT 2: @ts-ignore used to suppress a legitimate type error.
// The variable below is typed as `number` but assigned a `string`.
// @ts-ignore: intentional type mismatch for fixture defect demonstration
export const MAGIC_VALUE: number = "not-a-number";

/**
 * Returns the display name for a record.
 *
 * DEFECT 3: `console.log` in library code (should use a proper logger).
 */
export function getDisplayName(record: Record<string, unknown>): string {
  // DEFECT 3: console.log in library code
  console.log("getDisplayName called with:", record);
  const name = record["name"];
  if (typeof name === "string") {
    return name;
  }
  return "(unknown)";
}

/** Formats a list of items as a bullet-point string. */
export function formatList(items: string[]): string {
  return items.map((item) => `• ${item}`).join("\n");
}

/** Computes the sum of an array of numbers. */
export function sum(values: number[]): number {
  return values.reduce((acc, v) => acc + v, 0);
}

/** Clamps a value between min and max. */
export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/** Parses a comma-separated string into a trimmed string array. */
export function parseCsv(line: string): string[] {
  return line.split(",").map((s) => s.trim());
}
