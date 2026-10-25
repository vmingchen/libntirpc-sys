# libntirpc-sys

Low-level bindings for the [libntirpc](https://github.com/nfs-ganesha/ntirpc)
library.

Some documentation for the library can be found in the Linux man pages
[rpc(3)](https://linux.die.net/man/3/rpc) and
[xdr(3)](https://linux.die.net/man/3/xdr). However, the doc has some
inconsistency with the library, so we should use the doc as a reference not as a
definitive guide.

Its implementation is adapted from the
[libtirpc-sys](https://crates.io/crates/libtirpc-sys) library.

## Examples

The [examples](examples/) directory contains a small "hello world" RPC
client-server pair. The client uses a safe synchronous string RPC wrapper:

```rust
let reply = libntirpc_sys::client::call_string(
    c"127.0.0.1", 0x3000_0001, 1, 1, c"Alice", c"udp",
)?;
println!("{}", reply.as_c_str().to_string_lossy());
```

`client::call_string` initializes a client runtime once, borrows its input, and
returns an owned `XdrString` that frees the result on drop. It also cleans up
results on RPC errors. This focused API supports string arguments/results,
AUTH_NONE, and ntirpc's default timeout/retries. Independent calls can run
concurrently after one-time initialization. The runtime remains live for the
process lifetime. An active reply decoder finishes before a call returns, even
when it runs past the nominal timeout.
Use raw bindings for custom server/runtime initialization, authentication, or
other wire types. Raw FFI code must preserve the managed runtime and allocator
invariants when sharing a process with this wrapper.

The pair needs the `rpcbind` daemon, which is used to register the server's
program number and for the client to look it up:

```sh
sudo apt install rpcbind
```

Make sure it is running (restart it if the server fails to register):

```sh
sudo systemctl restart rpcbind
```

Run the server in one terminal:

```sh
cargo run --example hello_server
```

It should print `hello server: prog=0x30000001 vers=1 on udp` and register
with rpcbind (check with `rpcinfo -p`). Then, in another terminal, call it:

```sh
cargo run --example hello_client -- Alice 127.0.0.1
```

which prints `Hello, Alice!`. Both arguments are optional (`name` defaults to
`world`, `host` to `127.0.0.1`).

## Safe XDR strings

For memory-backed XDR strings, `xdr::MemoryEncoder` and `xdr::MemoryDecoder`
provide safe operations with owned, aligned buffers and fixed stream modes:

```rust
use libntirpc_sys::xdr::{MemoryDecoder, MemoryEncoder};

let mut encoder = MemoryEncoder::new(32)?;
encoder.encode_string(c"hello")?;
let mut decoder = MemoryDecoder::new(encoder.as_bytes())?;
let text = decoder.decode_string()?;
assert_eq!(text.as_c_str(), c"hello");
```

Decoded `XdrString` values own their ntirpc allocation and free it on drop;
their borrowed `CStr` views cannot outlive them. Encoding borrows a `CStr`
without taking ownership. Strings need not be UTF-8 and retain C's semantics
for embedded NUL bytes. Failures may advance the stream cursor.

`encoder.as_bytes()` borrows output without allocating or copying; use
`encoder.as_bytes().to_vec()` when an independent owned copy is needed.
`encoder.reset()` reuses the output allocation, while `decoder.rewind()` reads the same input
again without a copy. `decoder.refill(bytes)` copies a new input into retained
storage and allocates only when its capacity must grow. Decoded strings remain
valid across resets, refills, and destruction of their decoder. The fuzz adapter
copies each input once and rewinds that decoder for all three decoding passes.

Network callbacks still require an unsafe boundary: `XdrString::decode_from_raw`
requires a valid, exclusively accessed XDR stream with live backing storage and
correct callbacks. `XdrString::from_raw` requires unique ownership of a valid
ntirpc allocation. An arbitrary `*mut XDR` or `char **` cannot prove these
properties, and `xdr_wrapstring` can encode, allocate into, overwrite, or free
its argument depending on the stream mode. Use the typed memory wrappers when
possible; raw FFI callbacks retain their explicit safety contracts.

## Native build

Normal Cargo builds compile the packaged ntirpc source based on v15.7 at commit
`ff0ee01fc4960a72f70f21ca6a778c0963732a7b`. No source is downloaded during
the build, and a system-installed libntirpc is neither discovered nor linked.
The archive is static and position-independent; RCU and optional Kerberos
libraries remain system dependencies. Linux is the supported native platform.

This revision contains [PR #414](https://github.com/nfs-ganesha/ntirpc/pull/414),
which validates decoded RPC reply verifiers inside ntirpc. The bindings no
longer replace transport callbacks or depend on a copied private layout.

This revision also contains [PR #416](https://github.com/nfs-ganesha/ntirpc/pull/416),
which zero-initializes GSS credential state through `mem_zalloc`, replacing our
process-global allocator hook. Both runtime fixes are now upstream. Local build
and correctness patches are documented in [vendor/UPSTREAM.md](vendor/UPSTREAM.md)
and [vendor/PATCHES.md](vendor/PATCHES.md). Upstream license notices are preserved.

Install the build dependencies on Ubuntu:

```sh
sudo apt install build-essential cmake clang libclang-dev pkg-config liburcu-dev
```

The optional `rpcsec-gss` feature additionally requires `libkrb5-dev`.
Monitoring, LTTng, TLS and RDMA are disabled. Cross compilation requires
`CMAKE_TOOLCHAIN_FILE`, target RCU/Kerberos packages and bindgen's target
sysroot arguments via `BINDGEN_EXTRA_CLANG_ARGS`; it is not yet validated.

docs.rs uses checked-in declarations and skips the native build.

## Testing and ownership

Reproducible allocation/throughput benchmarks are documented in
[benches/README.md](benches/README.md). Stateful refill/reset/failure-recovery
fuzzing instruments both Rust and bundled C with AddressSanitizer and leak
detection; see [fuzz/README.md](fuzz/README.md) and run
`bash tests/fuzz-xdr.sh 30` for a bounded campaign.

Handwritten Rust must document every unsafe block with a `// SAFETY:` comment
that explains pointer validity, ownership, lifetimes, synchronization, and other
relevant assumptions. Unsafe functions, including private callbacks, need a
`# Safety` contract. Cargo denies `unsafe_op_in_unsafe_fn` and `unused_unsafe`;
Clippy denies `undocumented_unsafe_blocks` and `missing_safety_doc`. CI checks
all targets and features. Generator-specific exceptions are confined to the
private bindings module, whose declarations are re-exported at the crate root;
raw FFI callers must still uphold the bundled C API's requirements.

This repository is the source of truth. vNFS consumes published crate releases
or an explicit Cargo path override during coordinated development. Run
`cargo test --all-features` and `cargo clippy --all-targets --all-features` here.
The vNFS RPCSEC_GSS integration suite exercises the source-built client against
a separate NFS server and KDC. Packaging and offline builds must also pass
before a new source pin is released.


Additional native correctness patches are listed in
[vendor/PATCHES.md](vendor/PATCHES.md). Run the example and native regression
checks as well:

```sh
cargo test --examples --all-features
bash tests/check-native-xdr.sh
bash tests/check-bindgen-env.sh
```
