# libntirpc-sys

Rust bindings for the [libntirpc](https://github.com/nfs-ganesha/ntirpc) library,
with safe synchronous RPC clients and XDR helpers alongside the raw C API.
Native builds require Linux and Rust 1.88 or newer.

Some documentation for the library can be found in the Linux man pages
[rpc(3)](https://linux.die.net/man/3/rpc) and
[xdr(3)](https://linux.die.net/man/3/xdr). These pages describe related RPC
implementations and differ from ntirpc in places. Consult the bundled headers
and Rust API documentation for the exact interfaces.

Its implementation is adapted from the
[libtirpc-sys](https://crates.io/crates/libtirpc-sys) library.

## Examples

The [examples](examples/) directory contains a small "hello world" RPC
client and server. Install the [native build dependencies](#native-build) before
running them. A safe `Client` stores the host, program, version, and transport
once; a typed `RpcCodec` defines the procedure number and request/reply encoding.

`Hello` below defines the client codec for procedure 1. It sends a name and
decodes the reply. The greeting logic runs in
[`hello_server.rs`](examples/hello_server.rs), which returns `Hello, <name>!`.
The server must be running as described below.

```rust
use std::ffi::CStr;

use libntirpc_sys::{
    client::{Client, RpcCodec},
    rpcproc_t,
    xdr::{self, XdrDecoder, XdrEncoder, XdrString},
};

struct Hello;

impl RpcCodec for Hello {
    const NUMBER: rpcproc_t = 1;
    type Request = CStr;
    type Reply = XdrString;

    fn encode(xdr: &mut XdrEncoder<'_>, name: &CStr) -> Result<(), xdr::Error> {
        xdr.encode_string(name)
    }

    fn decode(xdr: &mut XdrDecoder<'_>) -> Result<XdrString, xdr::Error> {
        xdr.decode_string()
    }
}

fn main() -> Result<(), libntirpc_sys::client::Error> {
    let client = Client::new(c"127.0.0.1", 0x3000_0001, 1, c"udp")?;
    for name in [c"Alice", c"Bob"] {
        let reply = client.call::<Hello>(name)?;
        println!("{}", reply.as_c_str().to_string_lossy());
    }
    Ok(())
}
```

For string-to-string procedures, the supplied `WrapString<N>` codec avoids
writing an implementation: replace the `Hello` struct and its `impl` with
`type Hello = libntirpc_sys::client::WrapString<1>;`. The existing
`client::call_string` function remains available.

Construction copies the host and transport names once and initializes the client
runtime. Connections are established lazily and cached per thread by ntirpc.
Calls borrow requests and encode/decode directly over network XDR streams,
without an intermediate buffer or a copy of the request. The safe stream views
provide `encode_string`/`decode_string` and `encode_u32`/`decode_u32`; codecs can
combine these into structured requests and replies. `XdrString` owns its native
allocation and frees it on drop, including when an RPC fails after decoding a
result.

Requests must be `Sync` and replies `Send`, because codecs may run on worker
threads. With panic unwinding enabled, codec panics are caught before crossing C
and resumed on the calling thread after callbacks finish. Builds using
`panic = "abort"` still abort on a panic. Nested managed RPCs on a codec's thread
return `client::Error::ReentrantCall`. Codecs may run more than once on retries.

The client uses AUTH_NONE and ntirpc's default timeout/retries. Independent calls
can run concurrently, including through a shared `Client`. Dropping a client
does not close ntirpc's per-thread cached connections or stop the process-lifetime
runtime. An active reply decoder finishes before a call returns, even when it
runs past the nominal timeout. Custom server/runtime initialization,
authentication, and wire operations beyond the safe stream API require raw
bindings. Raw FFI code sharing the managed runtime must not shut it down,
reconfigure it, replace allocator hooks, or invoke raw `rpc_call` concurrently
with managed calls.

The pair needs the `rpcbind` daemon, which is used to register the server's
program number and for the client to look it up:

```sh
sudo apt install rpcbind
```

On systems using systemd, start it with:

```sh
sudo systemctl start rpcbind
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

The `hello_client` example prints `Hello, Alice!`. Both arguments are optional
(`name` defaults to `world`, `host` to `127.0.0.1`).

## Safe XDR strings

For memory-backed XDR strings, `xdr::MemoryEncoder` and `xdr::MemoryDecoder`
provide safe operations with owned, aligned buffers and fixed stream modes:

```rust
use libntirpc_sys::xdr::{MemoryDecoder, MemoryEncoder};

fn main() -> Result<(), libntirpc_sys::xdr::Error> {
    let mut encoder = MemoryEncoder::new(32)?;
    encoder.encode_string(c"hello")?;
    let mut decoder = MemoryDecoder::new(encoder.as_bytes())?;
    let text = decoder.decode_string()?;
    assert_eq!(text.as_c_str(), c"hello");
    Ok(())
}
```

Decoded `XdrString` values own their ntirpc allocation and free it on drop;
their borrowed `CStr` views cannot outlive them. Encoding borrows a `CStr`
without taking ownership. Strings need not be UTF-8. Input `CStr` values contain
no embedded NUL bytes; if a decoded wire string contains a NUL, `as_c_str()`
exposes only the prefix before it. These helpers use `xdr_wrapstring`, which
limits each string to 9000 bytes in the bundled ntirpc. Failed operations may
advance the stream cursor.

`encoder.as_bytes()` borrows output without allocating or copying; use
`encoder.as_bytes().to_vec()` when an independent owned copy is needed.
`encoder.reset()` reuses the output allocation, while `decoder.rewind()` reads
the same input again without a copy. `MemoryDecoder::new` copies its input into
owned, aligned storage. `decoder.refill(bytes)` copies new input into retained
storage and allocates only when its capacity must grow. Each decoded string has
its own native allocation and remains valid across resets, refills, and
destruction of its decoder.

Managed RPC codecs receive safe `XdrEncoder`/`XdrDecoder` views. They can also
be tested using `encoder.encoder()` and `decoder.decoder()` on memory streams.
Custom raw network callbacks still require an unsafe boundary:
`XdrString::decode_from_raw` requires a valid, exclusively accessed XDR stream
with live backing storage and correct callbacks. `XdrString::from_raw` requires
unique ownership of a valid ntirpc allocation. An arbitrary `*mut XDR` or
`char **` cannot prove these properties, and `xdr_wrapstring` can encode,
allocate into, overwrite, or free its argument depending on the stream mode.
Use the safe stream wrappers where possible; raw FFI callbacks retain their
explicit safety contracts.

## Native build

Normal Cargo builds compile the packaged ntirpc source based on v15.7 at commit
`ff0ee01fc4960a72f70f21ca6a778c0963732a7b`, with additional backports and local
fixes. The build script does not download native source or link a
system-installed ntirpc. The archive is static and position-independent; RCU
and optional Kerberos libraries remain system dependencies. The source pin,
build changes, and correctness patches are recorded in
[vendor/UPSTREAM.md](vendor/UPSTREAM.md) and [vendor/PATCHES.md](vendor/PATCHES.md).
Upstream license notices are preserved.

Install the build dependencies on Ubuntu:

```sh
sudo apt install build-essential cmake clang libclang-dev pkg-config liburcu-dev
```

The optional `rpcsec-gss` feature additionally requires `libkrb5-dev`.
It enables native RPCSEC_GSS support and raw GSS bindings; the safe `Client`
continues to use AUTH_NONE. Monitoring, LTTng, TLS, and RDMA are disabled.
Cross compilation requires `CMAKE_TOOLCHAIN_FILE`, a target C compiler, target
RCU packages (and Kerberos when enabled), and bindgen's target sysroot arguments
via `BINDGEN_EXTRA_CLANG_ARGS`. Cross compilation has not yet been validated.

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

Run the Rust, example, and native regression checks with the build dependencies
above, `libkrb5-dev`, and `python3` installed:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
cargo test --examples --all-features
bash tests/check-native-xdr.sh
bash tests/check-bindgen-env.sh
```

This repository is the source of truth. vnfs consumes published crate releases
or an explicit Cargo path override during coordinated development. Its RPCSEC_GSS
integration suite exercises the source-built client against a separate NFS
server and Kerberos KDC. Packaging and offline builds must also pass before
publishing a crate release.
