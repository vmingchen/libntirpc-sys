# Sanitized XDR fuzzing

Install the normal native build dependencies plus Python 3, clang, ripgrep, and
nm/binutils, then install the fuzz tools:

```sh
rustup toolchain install nightly --component rust-src
cargo install cargo-fuzz --version 0.13.2 --locked
bash tests/fuzz-xdr.sh 30
```

The final argument is positive seconds **per target**. The runner builds in
`target/fuzz-asan`, verifies ASan and sanitizer-coverage symbols in the bundled
native archives, and runs `xdr_state` and `rpc_xdr`. Rust is instrumented through
cargo-fuzz's address sanitizer; `LIBNTIRPC_SYS_FUZZ_NATIVE=1` makes build.rs compile
the bundled C and C helpers with clang's `-fsanitize=address,fuzzer-no-link`.
Leak detection is enabled. Normal builds do not enable these flags. Keep this
environment variable confined to cargo-fuzz builds: instrumented C requires the
sanitizer/coverage runtimes supplied by the fuzz executable. System dependencies
such as RCU are not rebuilt with sanitizers.

`xdr_state` interprets up to 64 operations. Each command is one opcode byte,
two little-endian payload-length bytes, and the payload (clamped to available
bytes). The opcode modulo eight selects:

| Code | Operation |
| --- | --- |
| 0 | Refill the decoder with arbitrary wire bytes. |
| 1 | Compare a reused decode and cursor with a fresh decoder; retain successful strings. |
| 2 | Rewind the current decoder. |
| 3 | Encode payload bytes up to the first NUL; verify wire bytes, or reset and encode an empty string after failure. |
| 4 | Reset the encoder and check its output is empty. |
| 5 | Refill the decoder from borrowed encoded output. |
| 6 | Force oversized-length failures, rewind, then recover with a valid empty string. |
| 7 | Destroy and recreate both streams while retaining decoded strings. |

Saved strings are checked after every command; at most eight are retained.
The encoder holds 64 bytes, intentionally forcing partial-write failures.
Fresh/reused comparison detects state leaks; it is not an independent decoder
implementation. Successful encodings are also compared with a Rust-built XDR
wire representation. `rpc_xdr` independently exercises the raw call, reply, and
string decoders. The runner bounds input size, per-input timeout and RSS.

`seed_corpus.py` supplies deterministic seeds for growth/shrinkage, retained
owners, missing payload/padding, non-UTF-8 data, embedded NULs, and recovery after
failed operations. Corpus discoveries and crash artifacts are ignored by Git.
To reproduce a finding, use the same sanitizer configuration and the path cargo-
fuzz prints, for example:

```sh
LIBNTIRPC_SYS_FUZZ_NATIVE=1 cargo +nightly fuzz run xdr_state \
  --sanitizer address --target-dir target/fuzz-asan \
  fuzz/artifacts/xdr_state/crash-HASH
```

The GitHub workflow runs bounded smoke campaigns on pushes and PRs. A passing
campaign is coverage evidence, not a proof that the native decoders are bug-free.
