#!/usr/bin/env bash
# Exercise native C APIs from the same installed headers consumers receive.
set -euo pipefail
cd "$(dirname "$0")/.."
test_dir=$(mktemp -d)
trap 'rm -rf "$test_dir"' EXIT
cargo build --lib --all-features --message-format=json > "$test_dir/build.json"
native_out=$(python3 - "$test_dir/build.json" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    message = json.loads(line)
    if message.get("reason") == "build-script-executed" and "libntirpc-sys" in message["package_id"]:
        print(message["out_dir"])
        break
else:
    raise SystemExit("libntirpc-sys native build output not found")
PY
)
read -r -a dependency_flags <<< "$(pkg-config --cflags --libs liburcu-bp krb5-gssapi)"
for fixture in tests/native_*.c; do
    executable="$test_dir/$(basename "$fixture" .c)"
    "${CC:-cc}" -D_GNU_SOURCE -DINET6 \
    -I"$native_out/native-install/include/ntirpc" \
    -I"$native_out/native-install/include" \
    -I"$native_out/bundled-build" \
    "$fixture" "$native_out/native-install/lib/libntirpc.a" \
    "${dependency_flags[@]}" -lpthread -o "$executable"
    "$executable"
done
