# Changelog

## Unreleased

- Fix an IPv4 broadcast address overread and initialize broadcast address
  storage, with native IPv4/IPv6 regression coverage.
- Select stable Rust explicitly in the main CI job.
- Remove deleted vendored sources from incremental native builds using a source
  manifest, preserving generated files and unchanged source timestamps.
- Add separate allocation and throughput benchmarks, native/Rust allocation
  accounting, and state-machine fuzzing with native/Rust ASan coverage and leak
  detection. Gate allocation invariants and bounded fuzz campaigns in CI.
- Borrow encoded XDR output without copying, reuse encoder/decoder allocations
  through reset/rewind/refill, and copy each fuzz input once for all decoders.
- Add safe memory XDR string encoders/decoders and an owning `XdrString` that
  frees with ntirpc's allocator; reuse them in fuzzing and hello examples.
- Enforce unsafe block comments and unsafe function contracts, with generated
  binding exceptions confined to their own module.
- Keep fuzzing XDR stream self-pointers stable, avoid raw byte-slice casts and
  shared mutable free streams, and document FFI ownership and lifetime rules.
- Use Rust-owned request allocations and safe UDP socket setup in the examples;
  install server callbacks before enabling event dispatch and free decoded
  strings through ntirpc's allocator.

## 0.3.3

- Backport reviewed upstream transport lock/reference, epoll teardown,
  broadcast cleanup, and RPCSEC_GSS deadlock/leak fixes. Record source commits,
  overlapping fixes, and excluded PRs in `vendor/PATCHES.md`.
- Fix parent-scope CMake warnings while preserving the existing CMake minimum,
  correct legacy man-page type markup, and add native failure-path regressions.

## 0.3.2

- Fix XDR string ownership on decode failure, string allocation overflow,
  and array allocation validation against the decoded count.
- Initialize authentication padding in RPC call and reply encoders.
- Respect UNIX transport address lengths and reject malformed universal
  address port components and UNIX paths.
- Return protocol errors from the hello RPC examples.
- Preserve unchanged native source timestamps and support quoted bindgen
  arguments and target-specific bindgen environment overrides.
- Add native regression tests and exclude editor swap files from packages.

## 0.3.1

- Update the bundled ntirpc pin to upstream merge commit
  `ff0ee01fc4960a72f70f21ca6a778c0963732a7b`, which includes PR #416's GSS
  client authentication zero-initialization fix. This is identical to the
  runtime fix bundled in 0.3.0; the GSS allocation fix no longer needs a
  local patch.

## 0.3.0

- Build packaged ntirpc v15.7 (`69c40fecf260ff9172f6dd9d7b468b0a541808bc`)
  as a PIC static archive, without build-time downloads or a system ntirpc.
- Use upstream PR #414's decoded RPC reply-verifier fix; remove private-layout
  transport callback replacements and their public install/uninstall functions.
- Zero-initialize GSS credential state through `mem_zalloc`, replacing the
  process-global allocator hook. Retain this small patch until upstream merges it.
- Support optional RPCSEC_GSS, packaged-source/offline tests, docs.rs declarations,
  and Rust 1.88. Native builds currently support Linux only.

This is a breaking native API/build change. Consumers must use the bundled
headers and let this crate own linkage instead of independently linking ntirpc.
RCU and optional Kerberos remain system dependencies.
