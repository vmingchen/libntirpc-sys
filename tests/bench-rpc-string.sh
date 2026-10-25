#!/usr/bin/env bash
# Interpose only this benchmark's network/runtime boundary using GNU ld wrapping.
set -euo pipefail
cd "$(dirname "$0")/.."
export LIBNTIRPC_SYS_RPC_BENCH_MOCK=1
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-Wl,--wrap=svc_init -C link-arg=-Wl,--wrap=rpc_call"
cargo bench --bench rpc_string_overhead
