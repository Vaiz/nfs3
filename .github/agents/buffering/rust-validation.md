# Rust Execution

Implement/Verify use actual `rust/*` MCP schemas for Cargo, never guessed names or
silent shell substitutes. Follow [repository tool-use policy](../../copilot-instructions.md#tool-use).
Do not infer Rust MCP unavailability from a missing discovery tool alone. Distinguish
session-enforced invocation restrictions from observed Rust call failures. Report
the exact current blocker once; retry only when its prerequisite changes.

Minimal probe: cargo-metadata, workspace manifest, `no_deps: true`, offline locking
when supported. Distinguish dependency-resolution failure from tool/server failure.

- Follow repository validation gates. Read `.github/rust-toolchain.env`, manifest
  and `.github/workflows/rust.yml` for toolchains/MSRV/targets; pass through MCP.
- Verify is check-only; no automatic fixes or dependency changes.
- No toolchain/system-tool installation without approval. Cargo bench without an
  MCP equivalent needs an explicit shell exception; test mode is not timing evidence.
- Report unavailable targets/tools and security/license policy gaps; never invent
  a deny policy or weaken gates. Preserve only evidence needed for current checks.
