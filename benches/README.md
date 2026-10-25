# XDR allocation and throughput benchmarks

Run these separately so allocator instrumentation does not affect timing:

```sh
cargo bench --bench xdr_allocations
cargo bench --bench xdr_throughput
```

`xdr_allocations` runs in its own single-threaded process. It counts Rust
allocation/reallocation calls with a forwarding global allocator and counts
ntirpc string malloc/free calls with forwarding native hooks. Hooks are restored
after all decoded strings are dropped. For 16-, 1024-, and 8192-byte strings it
checks that warmed-up reusable streams perform zero Rust allocations and that
both paths allocate and free exactly one native string. Buffer growth is warmed
up before measurement; a refill exceeding retained capacity can still allocate.

`xdr_throughput` has no allocation counters or native hooks. It reports median
nanoseconds per encode/decode round trip and payload MiB/s over five samples,
alternating measurement order. `-- --quick` reduces iterations for smoke checks.
Every round trip verifies decoded bytes. Input construction and reusable stream
setup are outside the measurements. All streams and decoded owners are dropped
normally during measurement, including the native allocation's release.

The owned path constructs both streams, makes an explicit output Vec copy, and
copies it into aligned decoder storage on every round trip. The reused path
resets an existing encoder, borrows its output, and refills an existing decoder.
Both include native encoding, one input-buffer copy, string allocation/decoding,
and string cleanup. Results compare these two API usage patterns, not safe versus
unsafe code, and do not measure network RPC throughput. Payload throughput counts
each string once per round trip, rather than summing both encode/decode traffic.

Timings depend on the machine, allocator, compiler, and concurrent workload.
Only the allocation invariants are CI gates; throughput is reported without a
machine-specific performance threshold.

## Safe RPC boundary overhead

```sh
bash tests/bench-rpc-string.sh
```

This isolated benchmark uses GNU ld `--wrap` to replace `svc_init` and
`rpc_call` only in the benchmark executable. It starts no worker threads and
performs no network I/O. All paths run real ntirpc string encoding, decoding,
and cleanup; the raw path uses string-pointer callbacks and the safe path uses
`Client::call::<WrapString<1>>` and the compatible `client::call_string` helper.
Runtime initialization and client construction are warmed up before measurement.
All paths must perform zero Rust allocations and exactly one native decoded
string allocation per call. Input is borrowed and no staging copy is made.
Timings include allocator counters and are diagnostic, not network throughput
measurements or CI timing thresholds. The normal benchmark command skips this
fixture unless the helper script enables its linker wrapping.
