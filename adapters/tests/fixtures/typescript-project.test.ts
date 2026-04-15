import { describe, it, expect } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE_DIR = path.resolve(
  __dirname,
  "../../../tests/fixtures/typescript-project"
);

describe("typescript-project fixture", () => {
  it("fixture directory exists", () => {
    expect(fs.existsSync(FIXTURE_DIR)).toBe(true);
  });

  it("has valid tsconfig.json with strict: true", () => {
    const tsconfigPath = path.join(FIXTURE_DIR, "tsconfig.json");
    expect(fs.existsSync(tsconfigPath)).toBe(true);
    const tsconfig = JSON.parse(
      fs.readFileSync(tsconfigPath, "utf-8")
    ) as unknown;
    expect(tsconfig).toMatchObject({
      compilerOptions: { strict: true },
    });
  });

  it("has valid package.json with correct name and type", () => {
    const pkgPath = path.join(FIXTURE_DIR, "package.json");
    expect(fs.existsSync(pkgPath)).toBe(true);
    const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf-8")) as unknown;
    expect(pkg).toMatchObject({
      name: "typescript-project",
      type: "module",
    });
  });

  it("has TypeScript source files with type annotations", () => {
    const srcDir = path.join(FIXTURE_DIR, "src");
    expect(fs.existsSync(srcDir)).toBe(true);
    const tsFiles = fs
      .readdirSync(srcDir)
      .filter((f) => f.endsWith(".ts"));
    expect(tsFiles.length).toBeGreaterThanOrEqual(1);
    // Check that at least one file has interface or type declarations
    let hasAnnotations = false;
    for (const file of tsFiles) {
      const content = fs.readFileSync(path.join(srcDir, file), "utf-8");
      if (
        content.includes("interface ") ||
        content.includes("type ") ||
        content.includes(": string") ||
        content.includes(": number")
      ) {
        hasAnnotations = true;
        break;
      }
    }
    expect(hasAnnotations).toBe(true);
  });

  it("has exactly four source files: types, task-manager, filter-engine, repository", () => {
    const srcDir = path.join(FIXTURE_DIR, "src");
    const tsFiles = fs
      .readdirSync(srcDir)
      .filter((f) => f.endsWith(".ts"))
      .sort();
    expect(tsFiles).toEqual([
      "filter-engine.ts",
      "repository.ts",
      "task-manager.ts",
      "types.ts",
    ]);
  });

  it("types.ts exports TASK_STATUS and TASK_PRIORITY constants", () => {
    const content = fs.readFileSync(
      path.join(FIXTURE_DIR, "src", "types.ts"),
      "utf-8"
    );
    expect(content).toContain("TASK_STATUS");
    expect(content).toContain("TASK_PRIORITY");
    expect(content).toContain("export");
  });

  it("filter-engine.ts exports FilterEngine and FilterExpressionParser", () => {
    const content = fs.readFileSync(
      path.join(FIXTURE_DIR, "src", "filter-engine.ts"),
      "utf-8"
    );
    expect(content).toContain("export class FilterEngine");
    expect(content).toContain("export class FilterExpressionParser");
  });

  it("repository.ts exports TaskRepository class", () => {
    const content = fs.readFileSync(
      path.join(FIXTURE_DIR, "src", "repository.ts"),
      "utf-8"
    );
    expect(content).toContain("export class TaskRepository");
  });

  it("task-manager.ts exports TaskManager class", () => {
    const content = fs.readFileSync(
      path.join(FIXTURE_DIR, "src", "task-manager.ts"),
      "utf-8"
    );
    expect(content).toContain("export class TaskManager");
  });

  it("has test files in the tests directory", () => {
    const testsDir = path.join(FIXTURE_DIR, "tests");
    expect(fs.existsSync(testsDir)).toBe(true);
    const testFiles = fs
      .readdirSync(testsDir)
      .filter((f) => f.endsWith(".test.ts"));
    expect(testFiles.length).toBeGreaterThanOrEqual(3);
    expect(testFiles).toContain("task-manager.test.ts");
    expect(testFiles).toContain("filter-engine.test.ts");
    expect(testFiles).toContain("repository.test.ts");
  });

  it("source files use ES module import/export syntax", () => {
    const srcDir = path.join(FIXTURE_DIR, "src");
    const files = fs
      .readdirSync(srcDir)
      .filter((f) => f.endsWith(".ts"));
    let hasImport = false;
    let hasExport = false;
    for (const file of files) {
      const content = fs.readFileSync(path.join(srcDir, file), "utf-8");
      if (content.includes("import ")) hasImport = true;
      if (content.includes("export ")) hasExport = true;
    }
    expect(hasImport).toBe(true);
    expect(hasExport).toBe(true);
  });

  it("types.ts contains Task interface definition", () => {
    const content = fs.readFileSync(
      path.join(FIXTURE_DIR, "src", "types.ts"),
      "utf-8"
    );
    expect(content).toContain("export interface Task");
    expect(content).toContain("id: string");
    expect(content).toContain("title: string");
  });

  it("tsconfig.json has NodeNext module resolution", () => {
    const tsconfigPath = path.join(FIXTURE_DIR, "tsconfig.json");
    const tsconfig = JSON.parse(
      fs.readFileSync(tsconfigPath, "utf-8")
    ) as unknown;
    expect(tsconfig).toMatchObject({
      compilerOptions: {
        module: "NodeNext",
        moduleResolution: "NodeNext",
      },
    });
  });

  it("source files use .js extension for local imports (NodeNext compat)", () => {
    const srcDir = path.join(FIXTURE_DIR, "src");
    const files = fs
      .readdirSync(srcDir)
      .filter((f) => f.endsWith(".ts"));
    for (const file of files) {
      const content = fs.readFileSync(path.join(srcDir, file), "utf-8");
      // Files that import from local modules should use .js extension
      const localImports = content.match(/from "\.\//g);
      if (localImports !== null) {
        // If there are local imports, at least one should use .js
        const jsLocalImports = content.match(/from "\.\/.*\.js"/g);
        expect(
          jsLocalImports,
          `${file} has local imports but none use .js extension`
        ).not.toBeNull();
      }
    }
  });

  it("task-manager.ts re-exports domain error types", () => {
    const content = fs.readFileSync(
      path.join(FIXTURE_DIR, "src", "task-manager.ts"),
      "utf-8"
    );
    expect(content).toContain("TaskNotFoundError");
    expect(content).toContain("ValidationError");
    expect(content).toContain("QueryError");
  });
});
