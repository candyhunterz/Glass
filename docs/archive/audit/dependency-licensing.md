# Glass Dependency Licensing Audit

**Date:** 2026-03-18
**Auditor:** Automated prelaunch audit
**Scope:** All direct and transitive dependencies in Cargo.lock (532 packages)
**Glass Version:** 2.5.0

## Summary

Glass is licensed under MIT. The vast majority of its 532 dependencies use MIT-compatible
permissive licenses (MIT, Apache-2.0, BSD, ISC, Zlib, CC0). **No critical blocking issues
were found**, but there are several items requiring attention before launch:

- **2 MPL-2.0 transitive dependencies** (weak copyleft — file-level, not project-level)
- **1 dependency with GPL-2.0 as a license option** (choosable; Apache-2.0 alternative exists)
- **`ring` uses AND-combined licensing** (Apache-2.0 AND ISC — both apply simultaneously)
- **Internal crates missing `license` field** in their Cargo.toml (housekeeping)
- **OpenSSL linked transitively via git2** on non-Windows platforms (license considerations)

---

## Glass License

| Field | Value |
|-------|-------|
| License file | `LICENSE` at repo root |
| License type | MIT |
| Copyright | 2026 Glass Contributors |
| Cargo.toml `license` | `"MIT"` |
| Verdict | **Valid standard MIT license text. Confirmed.** |

---

## Direct Dependencies by Crate

### Root Binary (`glass`)

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| alacritty_terminal | =0.25.1 | Apache-2.0 | Yes |
| winit | 0.30.13 | Apache-2.0 | Yes |
| wgpu | 28.0.0 | MIT OR Apache-2.0 | Yes |
| glyphon | 0.10.0 | MIT OR Apache-2.0 OR Zlib | Yes |
| pollster | 0.4.0 | Apache-2.0 OR MIT | Yes |
| arboard | 3.6.1 | MIT OR Apache-2.0 | Yes |
| clap | 4.5.60 | MIT OR Apache-2.0 | Yes |
| tokio | 1.50.0 | MIT | Yes |
| tracing | 0.1.44 | MIT | Yes |
| tracing-subscriber | 0.3.22 | MIT | Yes |
| anyhow | 1.0.102 | MIT OR Apache-2.0 | Yes |
| memory-stats | 1.2.0 | MIT OR Apache-2.0 | Yes |
| chrono | 0.4.44 | MIT OR Apache-2.0 | Yes |
| dirs | 6.0.0 | MIT OR Apache-2.0 | Yes |
| image | 0.25.9 | MIT OR Apache-2.0 | Yes |
| serde_json | 1.0.149 | MIT OR Apache-2.0 | Yes |
| uuid | 1.22.0 | Apache-2.0 OR MIT | Yes |
| regex | 1.12.3 | MIT OR Apache-2.0 | Yes |
| ureq | 3.2.0 | MIT OR Apache-2.0 | Yes |
| notify | 8.2.0 | CC0-1.0 | Yes |
| tempfile | 3.x | MIT OR Apache-2.0 | Yes |
| tracing-chrome | 0.7 (optional) | MIT | Yes |
| winresource | 0.1.30 (build) | MIT | Yes |

### glass_core

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| serde | 1.0.228 | MIT OR Apache-2.0 | Yes |
| toml | 0.9.12 | MIT OR Apache-2.0 | Yes |
| notify | 8.0 | CC0-1.0 | Yes |
| ureq | 3 | MIT OR Apache-2.0 | Yes |
| semver | 1.0.27 | MIT OR Apache-2.0 | Yes |

### glass_terminal

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| alacritty_terminal | =0.25.1 | Apache-2.0 | Yes |
| arboard | 3.6.1 | MIT OR Apache-2.0 | Yes |
| url | 2.5.8 | MIT OR Apache-2.0 | Yes |
| polling | 3.11.0 | Apache-2.0 OR MIT | Yes |
| vte | 0.15 | Apache-2.0 OR MIT | Yes |

### glass_renderer

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| wgpu | 28.0.0 | MIT OR Apache-2.0 | Yes |
| glyphon | 0.10.0 | MIT OR Apache-2.0 OR Zlib | Yes |
| bytemuck | 1.25.0 | Zlib OR Apache-2.0 OR MIT | Yes |

### glass_history

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| rusqlite | 0.38.0 | MIT | Yes |
| strip-ansi-escapes | 0.2.1 | Apache-2.0 OR MIT | Yes |

### glass_snapshot

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| blake3 | 1.8.3 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | Yes |
| ignore | 0.4.25 | MIT OR Unlicense | Yes |

### glass_mcp

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| rmcp | 1.1.1 | Apache-2.0 | Yes |
| schemars | 1.2.1 | MIT | Yes |
| similar | 2.7.0 | Apache-2.0 | Yes |

### glass_coordination

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 | Yes |
| windows-sys | 0.59 | MIT OR Apache-2.0 | Yes |
| libc | 0.2 | MIT OR Apache-2.0 | Yes |

### glass_agent

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| git2 | 0.20.4 | MIT OR Apache-2.0 | Yes |
| diffy | 0.4.2 | MIT OR Apache-2.0 | Yes |

### glass_errors

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| regex | 1 | MIT OR Apache-2.0 | Yes |

### glass_soi

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| (same as glass_errors, plus anyhow) | | | Yes |

### glass_feedback

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| regex | 1 | MIT OR Apache-2.0 | Yes |

### glass_scripting

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| rhai | 1.24.0 | MIT OR Apache-2.0 | Yes |

### glass_pipes

| Dependency | Version | License | Compatible? |
|------------|---------|---------|-------------|
| shlex | 1.3.0 | MIT OR Apache-2.0 | Yes |

---

## Flagged Dependencies

### 1. `option-ext` v0.2.0 — MPL-2.0 (Medium)

| Field | Detail |
|-------|--------|
| License | MPL-2.0 |
| Pulled in by | `dirs` -> `dirs-sys` -> `option-ext` |
| Used by | Nearly every Glass crate (via `dirs`) |
| Risk | **Medium** — MPL-2.0 is "weak copyleft." It requires that modifications *to the MPL-licensed file(s) themselves* be shared under MPL-2.0. It does NOT require your entire project to be open-sourced. Since Glass is MIT and open-source anyway, this is a non-issue in practice. For binary distribution, no action needed beyond including the license notice. |
| Action | No blocking issue. Include MPL-2.0 license text in attribution notices. |

### 2. `smartstring` v1.0.1 — MPL-2.0+ (Medium)

| Field | Detail |
|-------|--------|
| License | MPL-2.0+ (MPL 2.0 or later) |
| Pulled in by | `rhai` -> `smartstring` |
| Used by | `glass_scripting` |
| Risk | **Medium** — Same weak copyleft as above. File-level copyleft only. Compatible with MIT projects. The "+" means any later MPL version also accepted. |
| Action | No blocking issue. Include MPL-2.0 license text in attribution notices. |

### 3. `self_cell` v1.2.2 — Apache-2.0 OR GPL-2.0-only (Low)

| Field | Detail |
|-------|--------|
| License | Apache-2.0 OR GPL-2.0-only |
| Pulled in by | `glyphon` -> `cosmic-text` -> `self_cell` |
| Used by | `glass_renderer` |
| Risk | **Low** — This is an OR license (choose one). **Choose Apache-2.0**, which is fully MIT-compatible. The GPL-2.0 option exists but does not need to be selected. |
| Action | Document that Apache-2.0 is the chosen license for this dependency. |

### 4. `ring` v0.17.14 — Apache-2.0 AND ISC (Low)

| Field | Detail |
|-------|--------|
| License | Apache-2.0 AND ISC (both apply simultaneously) |
| Pulled in by | `ureq` -> `rustls` -> `ring` |
| Used by | TLS connections in `ureq` (usage API polling) |
| Risk | **Low** — Both Apache-2.0 and ISC are permissive and MIT-compatible. The AND means you must comply with both, which is straightforward. Apache-2.0 includes a patent grant. ISC is essentially equivalent to MIT. |
| Action | Include both Apache-2.0 and ISC license texts in attribution notices. |

### 5. `clipboard-win` v5.4.1 — BSL-1.0 (Low)

| Field | Detail |
|-------|--------|
| License | BSL-1.0 (Boost Software License 1.0) |
| Pulled in by | `arboard` -> `clipboard-win` |
| Used by | Windows clipboard support |
| Risk | **Low** — BSL-1.0 is a permissive license, fully compatible with MIT. It only requires that the license text be included in source distributions (not binary distributions). |
| Action | Include BSL-1.0 text if distributing source. |

### 6. `r-efi` v5.3.0/v6.0.0 — Apache-2.0 OR LGPL-2.1-or-later OR MIT (Not Applicable)

| Field | Detail |
|-------|--------|
| License | Apache-2.0 OR LGPL-2.1-or-later OR MIT |
| Pulled in by | Conditional platform target (EFI only) |
| Risk | **Not Applicable** — Only compiled for UEFI targets. Never included in Windows, macOS, or Linux builds. Even if it were, the MIT option can be chosen. |
| Action | None required. |

### 7. `webpki-roots` v1.0.6 — CDLA-Permissive-2.0 (Low)

| Field | Detail |
|-------|--------|
| License | CDLA-Permissive-2.0 (Community Data License Agreement — Permissive) |
| Pulled in by | `ureq` -> `webpki-roots` |
| Used by | TLS root certificate store for HTTPS connections |
| Risk | **Low** — CDLA-Permissive-2.0 is a data-focused permissive license. It permits free use, modification, and distribution. This crate contains Mozilla root CA certificates, and the CDLA-Permissive license is specifically designed for such data sets. Fully compatible with MIT. |
| Action | Include CDLA-Permissive-2.0 notice in attribution. |

### 8. OpenSSL (via `git2` -> `libgit2-sys` -> `openssl-sys`) (Medium)

| Field | Detail |
|-------|--------|
| License | OpenSSL 3.0+: Apache-2.0. OpenSSL 1.x: dual OpenSSL/SSLeay license |
| Pulled in by | `git2` -> `libgit2-sys` -> `openssl-sys` |
| Used by | `glass_agent` (git worktree operations) |
| Risk | **Medium** — On Linux/macOS, `openssl-sys` links against the system's OpenSSL. If the system has OpenSSL 3.0+, it is Apache-2.0 (compatible). If the system has OpenSSL 1.x, the legacy dual license applies, which has an advertising clause similar to old BSD. On Windows, `git2` typically uses Windows native crypto (SChannel) rather than OpenSSL. |
| Action | (1) Verify that Windows builds do not link OpenSSL (they likely use SChannel via `git2`'s default features). (2) For Linux .deb packaging, document that the system OpenSSL is dynamically linked and its license applies. (3) Consider whether `git2` can be compiled with the `vendored-openssl` feature to guarantee OpenSSL 3.0+ (Apache-2.0). |

### 9. `libssh2-sys` (via `git2` -> `libgit2-sys` -> `libssh2-sys`) (Low)

| Field | Detail |
|-------|--------|
| License | MIT OR Apache-2.0 (Rust binding). libssh2 itself: BSD-3-Clause |
| Pulled in by | `git2` -> `libgit2-sys` |
| Risk | **Low** — BSD-3-Clause is permissive and MIT-compatible. |
| Action | Include BSD-3-Clause notice for libssh2 in attribution. |

---

## Copyleft Risk Assessment

| License | Deps | Copyleft Type | Risk to Glass |
|---------|------|---------------|---------------|
| MPL-2.0 | option-ext, smartstring | Weak (file-level) | **Low** — Only modifications to those specific files must remain MPL-2.0. Does not infect the rest of the project. Glass is open-source anyway. |
| GPL-2.0 (as option) | self_cell | Strong (if chosen) | **None** — Apache-2.0 alternative available; choose that. |
| LGPL-2.1+ (as option) | r-efi | Strong (if chosen) | **None** — MIT alternative available; only for EFI targets. |
| AGPL | (none) | N/A | **None found.** |
| SSPL | (none) | N/A | **None found.** |

**Verdict: No copyleft infection risk.** All copyleft-licensed dependencies either offer a permissive
alternative (OR licensing) or are weak copyleft (MPL-2.0 file-level scope).

---

## Patent Concerns

| Dependency | License | Patent Clause |
|------------|---------|---------------|
| ring | Apache-2.0 AND ISC | Apache-2.0 includes explicit patent grant (Section 3). Positive for Glass — provides patent protection for cryptographic operations. |
| alacritty_terminal | Apache-2.0 | Apache-2.0 patent grant applies. |
| winit | Apache-2.0 | Apache-2.0 patent grant applies. |
| rmcp | Apache-2.0 | Apache-2.0 patent grant applies. |
| All Apache-2.0 deps | Apache-2.0 | All include patent grant. |

**Verdict: No patent concerns.** Apache-2.0's patent grant is protective, not restrictive.
No dependencies use patent-encumbered or FRAND-licensed technology.

---

## Attribution Requirements

When distributing Glass as a binary, the following attribution is required:

### Must Include in Binary Distribution

1. **Glass MIT license** — the `LICENSE` file
2. **Apache-2.0 license text** — required by all Apache-2.0-licensed dependencies (alacritty_terminal, winit, rmcp, similar, and ~18 others that are Apache-2.0 only)
3. **MPL-2.0 license text** — required by option-ext and smartstring
4. **ISC license text** — required by ring (Apache-2.0 AND ISC)
5. **BSD-3-Clause notice** — required by subtle, tiny-skia, tiny-skia-path
6. **BSD-2-Clause notice** — required by arrayref
7. **BSL-1.0 text** — required by clipboard-win (source distributions)
8. **CDLA-Permissive-2.0 notice** — required by webpki-roots
9. **Unicode-3.0 license** — required by unicode-ident and ICU crates

### For dual/triple-licensed crates (MIT OR Apache-2.0, etc.)

When a dependency offers MIT as an option, choosing MIT simplifies attribution (MIT only
requires inclusion of the copyright notice and permission notice). For the ~297 crates
licensed "Apache-2.0 OR MIT", choosing MIT reduces the attribution burden.

### Recommended Approach

Generate a `THIRD-PARTY-LICENSES` file using `cargo-about` or `cargo-license --json` that
aggregates all dependency licenses. Include this file alongside the Glass binary in all
distribution channels (GitHub releases, .deb packages, installers).

---

## Internal Crate Licensing Housekeeping

The following internal Glass crates are **missing the `license` field** in their Cargo.toml:

- glass_core
- glass_terminal
- glass_renderer
- glass_mux
- glass_history
- glass_snapshot
- glass_pipes
- glass_mcp
- glass_coordination
- glass_errors
- glass_soi
- glass_agent
- glass_feedback
- glass_scripting

While these are internal crates not published to crates.io, adding `license = "MIT"` to each
is best practice. It ensures consistency, makes `cargo license` output clean, and prevents
confusion if any crate is ever published separately.

---

## License Distribution Summary (All 532 Packages)

| License Category | Count | Notes |
|------------------|-------|-------|
| MIT OR Apache-2.0 | 297 | Standard Rust dual-license |
| MIT only | 104 | Including tokio, tracing, rusqlite |
| Apache-2.0 only | 18 | Including alacritty_terminal, winit, rmcp |
| Apache-2.0 OR MIT OR Zlib | 17 | Including bytemuck, glyphon |
| Unicode-3.0 | 18 | ICU/Unicode crates |
| MIT OR Unlicense | 9 | regex ecosystem |
| CC0-1.0 | 3 | notify, hexf-parse, tiny-keccak |
| ISC | 5 | inotify, libloading, rustls-webpki, untrusted |
| BSD-3-Clause | 3 | subtle, tiny-skia |
| Zlib | 3 | foldhash, slotmap |
| BSD-2-Clause | 1 | arrayref |
| BSL-1.0 | 2 | clipboard-win, error-code |
| MPL-2.0 / MPL-2.0+ | 2 | option-ext, smartstring |
| Apache-2.0 AND ISC | 1 | ring |
| Apache-2.0 OR GPL-2.0 | 1 | self_cell |
| CDLA-Permissive-2.0 | 1 | webpki-roots |
| N/A (internal) | 14 | Glass workspace crates |
| Other permissive combos | ~33 | Various multi-license permissive |

---

## Recommendations

### Before Launch (Priority Order)

1. **Generate THIRD-PARTY-LICENSES file** — Use `cargo-about` to produce a comprehensive
   attribution file. Bundle it with every binary distribution.
   ```bash
   cargo install cargo-about
   cargo about generate about.hbs > THIRD-PARTY-LICENSES
   ```

2. **Add `license = "MIT"` to all internal crates** — All 14 workspace crates are missing
   this field. Add it for consistency and `cargo license` cleanliness.

3. **Document license choice for `self_cell`** — Add a note (in THIRD-PARTY-LICENSES or a
   similar file) that Apache-2.0 is the chosen license for self_cell, not GPL-2.0.

4. **Verify OpenSSL linkage on each platform** — Run `ldd` (Linux) or `otool -L` (macOS)
   on the release binary to confirm OpenSSL version. On Windows, confirm SChannel is used
   instead of OpenSSL.

### Post-Launch (Ongoing)

5. **Add `cargo-deny` to CI** — This tool can enforce license policies automatically on
   every PR. Configure it to reject GPL, AGPL, SSPL, and any other unwanted licenses.
   ```bash
   cargo install cargo-deny
   cargo deny init   # creates deny.toml
   cargo deny check licenses
   ```

6. **Monitor `alacritty_terminal` license** — Currently Apache-2.0 only (not dual-licensed).
   While compatible with MIT, it means Glass cannot relicense the combined work under MIT
   alone — the Apache-2.0 terms must be honored. This is fine but worth noting.

7. **Consider vendoring OpenSSL** — If Linux binary distribution is planned, using
   `git2`'s `vendored-openssl` feature ensures OpenSSL 3.0 (Apache-2.0) is bundled rather
   than relying on the system's potentially older OpenSSL 1.x with its legacy license.

---

## Final Verdict

**PASS — No blocking licensing issues for launch.**

All dependencies use licenses compatible with MIT. The two MPL-2.0 transitive dependencies
are weak copyleft and pose no practical risk, especially for an open-source project.
The primary action items are generating a proper attribution file and adding `cargo-deny`
to CI for ongoing license compliance.
