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
