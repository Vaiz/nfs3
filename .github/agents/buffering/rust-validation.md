# Rust Execution Rules

For the implementation and verification agents only:

- Follow repository validation requirements using the configured Rust MCP tools
  for Cargo operations. Do not silently substitute shell Cargo commands.
- Read `.github/rust-toolchain.env` for pinned CI/formatting toolchains and the
  workspace manifest/CI for MSRV. Pass the applicable toolchain through MCP.
- Use `.github/workflows/rust.yml` for required checks and target coverage. Report
  unavailable tools, unsupported targets, and pre-existing failures explicitly.
- If timing measurements require `cargo bench` without an MCP equivalent, request
  explicit authorization for that terminal exception. Benchmark test mode is not
  timing evidence.
- Do not install or update toolchains/system tools without user authorization.
- Report security/license policy gaps; do not invent a `deny.toml` policy or weaken
  repository gates as an incidental part of this redesign.