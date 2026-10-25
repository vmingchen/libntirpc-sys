#!/usr/bin/env bash
# Instrument Rust and the bundled C archive, verify that archive, then fuzz both.
set -euo pipefail
cd "$(dirname "$0")/.."
fuzz_seconds=${1:-30}
[[ "$fuzz_seconds" =~ ^[1-9][0-9]*$ ]] || { echo 'Usage: bash tests/fuzz-xdr.sh [positive seconds per target]' >&2; exit 2; }
fuzz_target_dir="$PWD/target/fuzz-asan"
export LIBNTIRPC_SYS_FUZZ_NATIVE=1
export ASAN_OPTIONS="${ASAN_OPTIONS:+$ASAN_OPTIONS:}detect_leaks=1:abort_on_error=1"
python3 fuzz/seed_corpus.py
cargo +nightly fuzz build --sanitizer address --target-dir "$fuzz_target_dir" --codegen-units 16
mapfile -t fuzz_archives < <(rg --files --no-ignore "$fuzz_target_dir" | rg '/native-install/lib/libntirpc\.a$|/libntirpc_helpers\.a$')
[[ ${#fuzz_archives[@]} -gt 0 ]] || { echo 'No native archive to verify' >&2; exit 1; }
for fuzz_archive in "${fuzz_archives[@]}"; do
    nm -u "$fuzz_archive" | rg '__asan_' > /dev/null
    nm -u "$fuzz_archive" | rg '__sanitizer_cov_' > /dev/null
done
echo "Verified ASan and coverage symbols in ${#fuzz_archives[@]} native archive(s)."
for fuzz_target in xdr_state rpc_xdr; do
    cargo +nightly fuzz run "$fuzz_target" --sanitizer address --target-dir "$fuzz_target_dir" --codegen-units 16 -- \
        -max_total_time="$fuzz_seconds" -max_len=16384 -timeout=10 -rss_limit_mb=2048
done
