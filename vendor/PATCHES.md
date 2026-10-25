# Additional local correctness patches

These changes apply to the bundled NTIRPC source and its installed headers.
Keep the behavioral regression tests when refreshing the upstream source.

- `src/clnt_bcast.c`: copy only `sizeof(struct sockaddr_in)` from IPv4
  broadcast addresses instead of reading a full `sockaddr_storage`. Zero the
  destination storage so IPv6 flowinfo and unused address bytes are initialized.
  Covered by exact-sized IPv4 allocations and poisoned IPv4/IPv6 destination
  allocations in `tests/native_broadcast.c`.

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
| [#419](https://github.com/nfs-ganesha/ntirpc/pull/419) | `0b55bc2670c7cad8b3e3c1eb173783e4ef1071c4` | Already present as the string allocation overflow patch above; no duplicate change. |
| [#420](https://github.com/nfs-ganesha/ntirpc/pull/420) | `116bd46c7e6cffb966ad1b6d624648fd1578904b` | Applied in full. The epoll-only union access belongs inside its preprocessor guard. |
| [#421](https://github.com/nfs-ganesha/ntirpc/pull/421) | `d1781db7d81324194eb79de07bf15e4a9e5cad19`, `560e4911a2b633bb5a7362fbf2ea6702d63cd1c8` | Applied both commits. All successful transport lookups now return locked, as documented and required by the create callers. Event consumers explicitly unlock, and validation/TLS consumers release lookup references. Audited every lookup call site. |
| [#422](https://github.com/nfs-ganesha/ntirpc/pull/422) | `6eefabf8e768c4fc4372b52aa81d5edfb3422a9e` | Applied in full. All three UDP create error exits must release the caller reference acquired by lookup. |
| [#423](https://github.com/nfs-ganesha/ntirpc/pull/423) | `088640ef748b4a165916a2913479fc464e90c443` | Applied in full. Failed client negotiation destroys its embedded auth state, replacement wire verifiers free their predecessor, RPC/status failures release decoded context/token buffers, and destruction destroys the mutex. |
| [#424](https://github.com/nfs-ganesha/ntirpc/pull/424) | `c65e119542a53aa4594e0e306434b4199696b96f` | Applied in full. Interfaces can have NULL addresses; resolution, conversion, socket, encoding, and reply paths also need the added cleanup. |
| [#425](https://github.com/nfs-ganesha/ntirpc/pull/425) | `df71803e66a710d4ca4a851eb8616d64253f38ad` | Applied in full. Hash deletion and unref take the GSS context mutex, so failed replies must unlock first. Hash removal is limited to completed negotiation, matching insertion. Also frees copied context handles on three accept failures. |
| [#426](https://github.com/nfs-ganesha/ntirpc/pull/426) | `9fff1c2635bdf3c6a3e398d08f1eb77a77782c24` | Applied the two remaining fixes: accept descriptor zero at teardown and release the extra channel lookup reference on repeated registration. Other hunks are covered by #418 and #421; retain #418's single cleanup owner and `max_events` reset. |
| [#384](https://github.com/nfs-ganesha/ntirpc/pull/384) | `d8c0949b40b6d58617171d8927ced7f194259f30` | Applied with adaptation. Guard parent-scope exports using `get_directory_property(... PARENT_DIRECTORY)`, which works with the declared CMake 3.6.2 minimum. The proposed `PROJECT_IS_TOP_LEVEL` needs CMake 3.21, as the upstream review points out. |
| [#260](https://github.com/nfs-ganesha/ntirpc/pull/260) | `4f5e2a0cf0e55effe62aa7dd928b9bc677265d08` | Applied in full. Uses the type macro for three struct references in legacy man pages. No runtime change. |

The following PRs were reviewed and excluded:

| PR | Reviewed head | Reason |
| --- | --- | --- |
| [#367](https://github.com/nfs-ganesha/ntirpc/pull/367) | `b1c305b7929649a91a856efdb8b2353d354b6763` | Incomplete traversal fix. The new check occurs after other early returns and still leaves a check/destroy race. The current vendor already obtains the next node before releasing its reference. Upstream review also requests atomic handling. Do not treat this change as a complete concurrency fix. |
| [#364](https://github.com/nfs-ganesha/ntirpc/pull/364) | `03cf420c01f717c057eda4b00e56ba757a5e8802` | RDMA is disabled in this crate. The series mixes fixes and refactoring; its callback cap separately reads and increments the active count, so concurrent callers can exceed the cap. Bounds checks also occur after the read-list loop condition dereferences the entry. It needs further work and RDMA runtime validation. |
| [#417](https://github.com/nfs-ganesha/ntirpc/pull/417) | `4ae160340f0b70f78b1e11aeaf62a4d3856f9ee4` | Overlaps #364 and has the same concerns. A tester reports new I/O hanging after completion queue requests. Requires RDMA hardware validation before backporting. |
| [#411](https://github.com/nfs-ganesha/ntirpc/pull/411) | `3b9c0e5aa0174f20981a8a058a8ac166d1df0e2f` | New Linux zero-copy transmit feature, not a targeted bug fix. Changes public struct layouts and adds completion/buffer lifetime machinery and application prerequisites. Outside this correctness backport. |

Behavioral regressions run through `bash tests/check-native-xdr.sh`:

- `native_transport.c`: existing-entry lookup lock ownership, same/different
  transport validation without reference growth, and repeated UDP create failures.
- `native_evchan.c`: events ownership, idempotent teardown with descriptor reuse,
  descriptor zero, and repeated registration without channel reference growth.
- `native_broadcast.c`: synthetic interfaces without addresses, failed address
  resolution, and failed multicast conversion with balanced allocation/free.
- `native_gss_client.c`: failed negotiation releases auth state and repeated
  verifier validation retains only the latest buffer.
- `native_gss_server.c`: injected display/MIC/checksum-length failures release
  copied contexts; failed replies after complete and continuing negotiation
  return without deadlock and destroy the context exactly once.

The GSS regressions inject mechanism failures and do not require a KDC. They
do not replace successful RPCSEC_GSS integration against a server and KDC.
TLS reference changes are reviewed but not exercised at runtime because TLS is
disabled. RDMA remains disabled. The native runner limits each fixture to 30
seconds so lock regressions fail instead of hanging CI.
