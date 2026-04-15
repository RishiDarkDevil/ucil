import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

// Root is set to the workspace root so that the acceptance-criterion command
//   pnpm --filter adapters vitest run adapters/tests/fixtures/typescript-project.test.ts
// resolves the file path correctly when pnpm cds into the adapters/ directory.
export default defineConfig({
  root: resolve(__dirname, ".."),
  test: {
    include: ["adapters/tests/**/*.test.ts"],
    environment: "node",
  },
});
