# DEC-0008: LSP diagnostics bridge uses Serena's MCP channel, not a literal shared socket

**Status**: accepted
**Date**: 2026-04-18
**Work-order**: WO-0014 (P1-W5-F03)
**Supersedes**: —

## Context

Master-plan §13.4 lines 1424–1431 describes the diagnostics bridge's
interaction with Serena as:

> When Serena is active as a P0 plugin, it manages the LSP server
> processes. The diagnostics bridge connects to these same server
> instances — no duplicate processes. Detection:
> 1. On daemon startup, check if Serena plugin is ACTIVE
> 2. **If active: query Serena for its managed LSP server PIDs/sockets,
>    connect directly**
> 3. If not active (degraded mode): spawn own LSP servers with the same
>    lifecycle management (lazy start, grace period shutdown, health
>    monitoring)

Feature `P1-W5-F03` reuses the same phrasing: *"connects to
Serena-managed LSP server instances via shared socket; no duplicate LSP
processes when Serena active."*

Taken literally this assumes Serena exposes an MCP tool (or an auxiliary
socket) that externalises the PIDs or Unix-domain-socket paths of the
language servers it is supervising so that a third party (UCIL) can
speak LSP directly to those servers. That capability **is not present
in upstream Serena** as of `v1.0.0` (the ref pinned in
`plugins/structural/serena/plugin.toml`). Serena's upstream README and
the `tools/list` response returned by `ucil plugin install serena --format
json` expose only the **semantic** LSP capabilities (`find_symbol`,
`find_references`, `go_to_definition`, hover, refactor, etc.) — it does
not advertise a tool that returns LSP server endpoints.

Two possible readings of the spec:

1. **Literal**: wait for Serena to add an endpoint-discovery tool
   upstream (or fork Serena to add one). Blocks the entire Week-5 LSP
   chain on external work we do not control.
2. **Semantic (this ADR)**: interpret *"shared socket"* as a
   *shared delegation channel* — i.e., the MCP stdio pipe that
   `PluginManager` already owns for the Serena plugin. The LSP
   diagnostics bridge does not open a second transport; when Serena is
   ACTIVE it delegates LSP requests to Serena via the existing MCP
   channel (calling Serena's high-level LSP-backed tools), so "no
   duplicate LSP processes" is satisfied because UCIL *never spawns LSP
   subprocesses of its own* while Serena is active.

## Decision

We adopt the **semantic interpretation**.

For `P1-W5-F03` (bridge skeleton) and all downstream LSP features in
Phase 1:

1. **Detection of Serena active state** is done by introspecting
   `ucil-daemon::plugin_manager::PluginManager` — specifically, the
   snapshot returned by `registered_runtimes()` is scanned for a runtime
   whose `manifest.plugin.name == "serena"` *and* whose `state ==
   PluginState::Active`. That boolean is the only input needed at the
   `P1-W5-F03` scope.
2. **Bridge construction API**: `LspDiagnosticsBridge::new(serena_managed:
   bool) -> Self`. The daemon (in a later integration WO, not P1-W5-F03)
   will compute `serena_managed` and pass it in. The bridge crate
   **does not depend on `ucil-daemon`** — cycle-free.
3. **No own LSP processes are spawned at `P1-W5-F03`**. The endpoint map
   (`HashMap<Language, LspEndpoint>`) is introduced empty; when
   `serena_managed = true` it stays empty (LSP responsibility is
   delegated to Serena); when `serena_managed = false` it stays empty
   *for now*, and `P1-W5-F07` fills it with spawned standalone-LSP
   endpoints.
4. **`P1-W5-F04`** (LSP JSON-RPC client) dispatches diagnostic /
   callHierarchy / typeHierarchy requests in two branches:
   - `serena_managed = true` → route through the existing MCP channel
     to Serena's high-level LSP-backed tools. The implementation detail
     of how the bridge reaches the PluginManager's Serena channel is
     reserved for `P1-W5-F04`'s WO — likely via a thin `SerenaClient`
     trait the bridge accepts in its constructor (added in `P1-W5-F04`,
     not here). `P1-W5-F03` carries only the `serena_managed` flag;
     the trait is the extension point for `P1-W5-F04`.
   - `serena_managed = false` → speak JSON-RPC directly to the
     standalone LSP subprocesses populated by `P1-W5-F07`.

## Rationale

* Avoids blocking UCIL on upstream-Serena work that is not part of the
  24-week delivery scope.
* Preserves the spec's **observable** invariants:
  * "no duplicate LSP processes when Serena active" — satisfied because
    UCIL does not spawn LSP subprocesses while Serena is active.
  * "connects to Serena's LSP servers" — satisfied semantically via the
    existing MCP channel; Serena's LSP servers ultimately serve the
    request.
  * Degraded mode (own LSP subprocesses) matches the spec exactly.
* Matches how other MCP-speaking tools integrate with Serena in the
  wild (agents talk to Serena's high-level tools rather than
  introspecting its internals).
* Keeps `ucil-lsp-diagnostics` **cycle-free** with `ucil-daemon`.

## Consequences

* **Bridge constructor shape** is frozen at
  `LspDiagnosticsBridge::new(serena_managed: bool)` for the duration of
  Phase 1. `P1-W5-F04` may add *additional* constructors (e.g.
  `with_serena_client`, `with_spawned_servers`) but must not break
  `new`.
* **Integration wiring** (computing `serena_managed` from
  `PluginManager::registered_runtimes()`) is reserved for a future WO
  — not part of `P1-W5-F03`. The WO's acceptance test exercises both
  bool values directly.
* **No new `ucil-daemon` dependency edge** from `ucil-lsp-diagnostics`.
* **Degraded-mode (own-server) spawning** is explicitly deferred to
  `P1-W5-F07`. `P1-W5-F03` ships with an empty endpoint map in both
  branches.
* **Master-plan wording mismatch** is noted here and does not require a
  spec amendment; the spec's observable invariants (no duplicate
  processes, diagnostics feed G7, call/type hierarchies feed G4) are
  preserved.

## Revisit trigger

Supersede this ADR if:
- Upstream Serena adds a tool like `list_lsp_servers` returning server
  PIDs/sockets (then the literal interpretation becomes implementable).
- The integration tests under `P1-W5-F08` reveal that delegating via
  Serena's MCP tools is too coarse-grained (e.g. we need raw
  `textDocument/diagnostic` responses rather than Serena's curated
  output). In that case, an ADR extending the bridge to spawn its *own*
  LSP servers *alongside* Serena (explicitly accepting the duplicate
  process cost) will be written.
