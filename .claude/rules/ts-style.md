# TypeScript style rules (UCIL)

## Toolchain
- `pnpm` workspace rooted at `adapters/`.
- Node 20+ LTS.
- `tsconfig.json` with `strict: true`, `noUncheckedIndexedAccess: true`, `exactOptionalPropertyTypes: true`.

## Lint & format
- Biome as the single formatter + linter (`biome format`, `biome check`).
- No ESLint/Prettier — Biome only to avoid tooling sprawl.

## Tests
- `vitest` per-package.
- Integration tests run real MCP servers over stdio — no Jest mocks of MCP transport.

## Types
- `any` is forbidden. `unknown` + narrowing is required.
- No `@ts-ignore` / `@ts-expect-error` without an inline rationale that references an ADR or issue id.
- Return types explicit on exported functions.

## Imports
- Prefer named imports. No default exports in library code.
- Imports sorted via Biome.

## Error handling
- Custom error classes extending `Error` with a `name` property.
- Top-level `async` handlers catch and log structured errors.

## Naming
- `camelCase` for variables/functions, `PascalCase` for types/classes, `SCREAMING_SNAKE` for constants.
- File names: `kebab-case.ts` for modules; `PascalCase.ts` for single-class modules.
