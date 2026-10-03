# Changelog

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
