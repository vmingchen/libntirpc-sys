# Additional local correctness patches

These changes apply to the bundled NTIRPC source and its installed headers.
Keep the behavioral regression tests when refreshing the upstream source.

- `ntirpc/rpc/xdr_inline.h`: preserve caller-owned string storage when decoding
  fails. Free only storage allocated by that decode attempt. Covered by the
  string regressions in `tests/xdr.rs`.
