# Dependency Licensing Implementation Plan (Branch 8 of 8)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure every crate declares its license, generate a third-party attribution file, and add CI enforcement to prevent GPL/AGPL/SSPL dependencies from entering the tree.

**Architecture:** Purely mechanical — add metadata fields, run tooling, commit artifacts.

**Tech Stack:** Rust, cargo-about, cargo-deny

**Branch:** `audit/dependency-licensing` off `master`

**Spec:** `docs/superpowers/specs/2026-03-18-prelaunch-audit-fixes-design.md` Branch 8

---

### Task 1: Add `license = "MIT"` to all 14 internal crates (LIC-1)

**Files:**
- Modify: `crates/glass_agent/Cargo.toml`
- Modify: `crates/glass_coordination/Cargo.toml`
- Modify: `crates/glass_core/Cargo.toml`
- Modify: `crates/glass_errors/Cargo.toml`
- Modify: `crates/glass_feedback/Cargo.toml`
- Modify: `crates/glass_history/Cargo.toml`
- Modify: `crates/glass_mcp/Cargo.toml`
- Modify: `crates/glass_mux/Cargo.toml`
- Modify: `crates/glass_pipes/Cargo.toml`
- Modify: `crates/glass_renderer/Cargo.toml`
- Modify: `crates/glass_scripting/Cargo.toml`
- Modify: `crates/glass_snapshot/Cargo.toml`
- Modify: `crates/glass_soi/Cargo.toml`
- Modify: `crates/glass_terminal/Cargo.toml`

- [ ] **Step 1: Create branch**

```bash
git checkout -b audit/dependency-licensing master
```

- [ ] **Step 2: Add `license = "MIT"` to each crate's `[package]` section**

In every file listed above, add `license = "MIT"` after the `edition` line. Example for `crates/glass_core/Cargo.toml`:

```toml
[package]
name = "glass_core"
version = "0.1.0"
edition = "2021"
license = "MIT"
```

All 14 crates get the identical `license = "MIT"` line. The root `Cargo.toml` already has it at line 77.

- [ ] **Step 3: Build to verify no breakage**

```bash
cargo build 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/*/Cargo.toml
git commit -m "chore(LIC-1): add license = MIT to all 14 internal crates

Root Cargo.toml already declared MIT but all workspace member
crates were missing the field. Required for cargo-deny and
crates.io publishing."
```

---

### Task 2: Generate THIRD-PARTY-LICENSES with cargo-about (LIC-2, LIC-3)

**Files:**
- Create: `about.toml` (cargo-about config)
- Create: `THIRD-PARTY-LICENSES` (generated output)

- [ ] **Step 1: Install cargo-about**

```bash
cargo install cargo-about
```

- [ ] **Step 2: Create `about.toml` config at repo root**

```toml
# cargo-about configuration
# See https://embarkstudios.github.io/cargo-about/
accepted = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Zlib",
    "BSL-1.0",
    "OpenSSL",
    "CC0-1.0",
    "MPL-2.0",
]

# self_cell uses Apache-2.0 — this is intentionally accepted.
# Apache-2.0 is compatible with MIT for downstream consumers.
```

- [ ] **Step 3: Generate the attribution file**

```bash
cargo about generate about.hbs > THIRD-PARTY-LICENSES
```

If the default template (`about.hbs`) is not available, use the built-in plaintext template:

```bash
cargo about generate -o THIRD-PARTY-LICENSES --format json | cargo about generate -
```

Or more simply, use the bundled template approach:

```bash
cargo about generate --format plaintext > THIRD-PARTY-LICENSES
```

Check the cargo-about docs for the exact invocation — the tool has evolved. The goal is a readable plaintext file listing every dependency, its license, and the license text.

- [ ] **Step 4: Verify `self_cell` appears with Apache-2.0 (LIC-3)**

Search the generated file for `self_cell` and confirm it lists `Apache-2.0`. This documents the license choice as required by the spec.

- [ ] **Step 5: Verify no OpenSSL linkage (LIC-4, LIC-6)**

On Windows (primary dev platform), Rust's `ureq` and TLS stack use SChannel (native), not OpenSSL. Verify:

```bash
cargo tree -i openssl 2>&1
# Should print nothing or "no crate found"
cargo tree -i openssl-sys 2>&1
# Should print nothing or "no crate found"
```

If either returns results, investigate which dependency pulls it in. On Windows builds, SChannel should be used via `native-tls` or `rustls`. Document the finding in the commit message.

- [ ] **Step 6: Commit**

```bash
git add about.toml THIRD-PARTY-LICENSES
git commit -m "chore(LIC-2/LIC-3): generate THIRD-PARTY-LICENSES via cargo-about

Adds about.toml config with accepted license list.
self_cell uses Apache-2.0, which is MIT-compatible.
LIC-4/LIC-6: verified no OpenSSL linkage — Windows uses SChannel,
Unix uses rustls/native-tls."
```

---

### Task 3: Add cargo-deny config and CI job (LIC-5)

**Files:**
- Create: `deny.toml` (cargo-deny config)
- Modify: `.github/workflows/ci.yml` (add deny job)

- [ ] **Step 1: Install cargo-deny**

```bash
cargo install cargo-deny
```

- [ ] **Step 2: Create `deny.toml` at repo root**

```toml
# cargo-deny configuration
# See https://embarkstudios.github.io/cargo-deny/

[licenses]
unlicensed = "deny"
copyleft = "deny"
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Zlib",
    "BSL-1.0",
    "OpenSSL",
    "CC0-1.0",
    "MPL-2.0",
]
deny = [
    "GPL-2.0",
    "GPL-3.0",
    "AGPL-1.0",
    "AGPL-3.0",
    "SSPL-1.0",
]
confidence-threshold = 0.8

[[licenses.clarify]]
name = "ring"
expression = "MIT AND ISC AND OpenSSL"
license-files = [{ path = "LICENSE", hash = 0xbd0eed23 }]

[bans]
multiple-versions = "warn"
wildcards = "allow"

[advisories]
unmaintained = "warn"
yanked = "warn"
notice = "warn"

[sources]
unknown-registry = "warn"
unknown-git = "warn"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []
```

Note: The `ring` clarify section may need its hash updated. Run `cargo deny check licenses` and if it reports a hash mismatch for `ring`, update the hash to the value it suggests. If `ring` is not in the dependency tree, remove the `[[licenses.clarify]]` block.

- [ ] **Step 3: Run cargo-deny locally to validate**

```bash
cargo deny check 2>&1
```

Fix any issues:
- If new licenses appear that are permissive but missing from the allow list, add them
- If the `ring` hash is wrong, update it per the error message
- Warnings about multiple versions or unmaintained crates are informational — they should not block CI

- [ ] **Step 4: Add cargo-deny job to CI**

In `.github/workflows/ci.yml`, add a new job after the `fmt` job:

```yaml
  deny:
    name: Dependency licenses
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2
```

This uses the official GitHub Action which handles installing cargo-deny automatically.

- [ ] **Step 5: Run full CI checks locally**

```bash
cargo fmt --all -- --check 2>&1
cargo clippy --workspace -- -D warnings 2>&1
cargo test --workspace 2>&1
cargo deny check 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add deny.toml .github/workflows/ci.yml
git commit -m "chore(LIC-5): add cargo-deny to reject GPL/AGPL/SSPL dependencies

deny.toml rejects copyleft licenses that are incompatible with MIT.
CI job uses EmbarkStudios/cargo-deny-action@v2 to enforce on every PR."
```

---

### Task 4: Final verification

- [ ] **Step 1: Verify all license fields present**

```bash
grep -L "license" crates/*/Cargo.toml
# Should return nothing — all crates now have the field
```

- [ ] **Step 2: Verify deny passes clean**

```bash
cargo deny check licenses 2>&1
```

- [ ] **Step 3: Verify THIRD-PARTY-LICENSES exists and is non-empty**

```bash
wc -l THIRD-PARTY-LICENSES
# Should be several hundred lines
```

- [ ] **Step 4: Summary — verify all items addressed**

Check off against the spec:
- [x] LIC-1: `license = "MIT"` in all 14 crates (Task 1)
- [x] LIC-2: THIRD-PARTY-LICENSES generated (Task 2)
- [x] LIC-3: self_cell Apache-2.0 documented (Task 2)
- [x] LIC-4: OpenSSL linkage verified (Task 2)
- [x] LIC-5: cargo-deny in CI with deny.toml (Task 3)
- [x] LIC-6: Windows SChannel verified (Task 2)
