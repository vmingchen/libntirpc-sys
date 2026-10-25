# Additional local correctness patches

These changes apply to the bundled NTIRPC source and its installed headers.
Keep the behavioral regression tests when refreshing the upstream source.

- `ntirpc/rpc/xdr_inline.h`: preserve caller-owned string storage when decoding
  fails. Free only storage allocated by that decode attempt. Covered by the
  string regressions in `tests/xdr.rs`.

- `ntirpc/rpc/xdr_inline.h`: check array allocation overflow against the
  decoded count and reject zero-sized elements. Covered by the inline C
  regressions in `tests/native_xdr.c`.

- `src/rpc_callmsg.c` and `src/rpc_dplx_msg.c`: initialize authentication
  padding in the inline call and reply encoders. Covered by the sentinel-buffer encoding regression
  in `tests/xdr.rs`, including maximum-size authentication bodies.

- `src/rpc_generic.c`: size UNIX universal-address output for the bounded
  pathname and respect the transport buffer's logical length. Covered by
  guard-allocation and short-buffer regressions in `tests/native_address.c`.

- `src/rpc_generic.c`: reject malformed decimal port octets and UNIX
  pathnames that would be truncated or are not absolute. Covered by invalid
  input and valid IPv4/IPv6/UNIX round trips in `tests/native_address.c`.

- `ntirpc/rpc/xdr_inline.h`: reject a string length whose terminating NUL
  would overflow the allocation size. The allocator-counting boundary test
  in `tests/native_xdr.c` uses only a four-byte input.

## Upstream PR review and backports (2026-10-25)

Reviewed all 15 open PRs returned by GitHub's live API. The base source pin in
[UPSTREAM.md](UPSTREAM.md) is unchanged. These are backports, not upstream merges;
commit IDs below identify the reviewed revisions. Source license notices and
authorship remain with the original commits.

| PR | Reviewed commit(s) | Applied change and assessment |
| --- | --- | --- |
| [#418](https://github.com/nfs-ganesha/ntirpc/pull/418) | `4cb5ec589390226d1ad79225616a1e60d5280160` | Applied in full. Channel destruction owns the events allocation and epoll descriptor, including setup failures. Removes the loop's stale descriptor close and duplicate cleanup. Also frees worker state when `pthread_create` fails (part of this commit, although omitted from its PR description). |
| [#420](https://github.com/nfs-ganesha/ntirpc/pull/420) | `116bd46c7e6cffb966ad1b6d624648fd1578904b` | Applied in full. The epoll-only union access belongs inside its preprocessor guard. |
| [#421](https://github.com/nfs-ganesha/ntirpc/pull/421) | `d1781db7d81324194eb79de07bf15e4a9e5cad19`, `560e4911a2b633bb5a7362fbf2ea6702d63cd1c8` | Applied both commits. All successful transport lookups now return locked, as documented and required by the create callers. Event consumers explicitly unlock, and validation/TLS consumers release lookup references. Audited every lookup call site. |
