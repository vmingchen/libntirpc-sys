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
client-server pair built directly on the raw bindings. The client issues a
single synchronous call:

```rust
use libntirpc_sys::*;

// Send `name`, receive the greeting (`wrap_string` adapts `xdr_wrapstring`
// to the generic `xdrproc_t` signature; see examples/hello_client.rs).
let stat = unsafe {
    rpc_call(
        host.as_ptr(), HELLO_PROG, HELLO_VERS, HELLO_PROC,
        Some(wrap_string),                       // encode the argument
        &mut name as *mut _ as *const c_void,
        Some(wrap_string),                       // decode the result
        &mut greeting as *mut _ as *mut c_void,
        c"udp".as_ptr(),                         // transport
    )
};
assert!(stat == clnt_stat_RPC_SUCCESS);
```

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

## Native build

Normal Cargo builds compile the packaged ntirpc v15.7 source at commit
`69c40fecf260ff9172f6dd9d7b468b0a541808bc`. No source is downloaded during
the build, and a system-installed libntirpc is neither discovered nor linked.
The archive is static and position-independent; RCU and optional Kerberos
libraries remain system dependencies. Linux is the supported native platform.

This revision contains [PR #414](https://github.com/nfs-ganesha/ntirpc/pull/414),
which validates decoded RPC reply verifiers inside ntirpc. The bindings no
longer replace transport callbacks or depend on a copied private layout.

Two local source patches are documented in [vendor/UPSTREAM.md](vendor/UPSTREAM.md):
static archive construction and zero-initialized GSS credential allocation.
The latter replaces our process-global allocator hook; PR #414 does not fix
that separate initialization defect. Upstream license notices are preserved.

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

This repository is the source of truth. vNFS consumes published crate releases
or an explicit Cargo path override during coordinated development. Run
`cargo test --all-features` and `cargo clippy --all-targets --all-features` here.
The vNFS RPCSEC_GSS integration suite exercises the source-built client against
a separate NFS server and KDC. Packaging and offline builds must also pass
before a new source pin is released.
