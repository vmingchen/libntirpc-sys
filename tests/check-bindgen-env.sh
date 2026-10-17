#!/usr/bin/env bash
# Regression: bindgen arguments may contain quoted paths with spaces.
set -euo pipefail
cd "$(dirname "$0")/.."
test_dir=$(mktemp -d)
trap 'rm -rf "$test_dir"' EXIT
mkdir "$test_dir/include dir"
printf '#define LIBNTIRPC_SYS_QUOTED_INCLUDE 1\n' > "$test_dir/include dir/extra.h"
BINDGEN_EXTRA_CLANG_ARGS="-include \"$test_dir/include dir/extra.h\"" \
    cargo check --lib --all-features

# The fallback stays fixed: only the target-specific variable changes between
# builds. Both spellings must invalidate Cargo's cached generated declarations.
printf "static const unsigned LIBNTIRPC_SYS_TARGET_VALUE = LIBNTIRPC_SYS_TARGET_FLAG;\n" > "$test_dir/target.h"
target=$(rustc -vV | sed -n 's/^host: //p')
dashed="BINDGEN_EXTRA_CLANG_ARGS_$target"
underscored="BINDGEN_EXTRA_CLANG_ARGS_${target//-/_}"
for variable in "$dashed" "$underscored"; do
    for value in 1 2; do
        env -u "$dashed" -u "$underscored" \
            BINDGEN_EXTRA_CLANG_ARGS=-DLIBNTIRPC_SYS_TARGET_FLAG=999 \
            "$variable=-DLIBNTIRPC_SYS_TARGET_FLAG=$value -include \"$test_dir/target.h\"" \
            cargo check --lib --all-features --message-format=json > "$test_dir/build.json"
        native_out=$(python3 tests/cargo-output.py "$test_dir/build.json")
        python3 - "$native_out/bindings.rs" "$value" <<'PY'
from pathlib import Path
import re
import sys
declaration = re.search(
    r"pub\s+const\s+LIBNTIRPC_SYS_TARGET_VALUE\s*:\s*[^=]+\s*=\s*([0-9]+)\s*;",
    Path(sys.argv[1]).read_text(),
)
assert declaration and declaration[1] == sys.argv[2], (
    f"stale target-specific declaration: expected {sys.argv[2]}, got {declaration}"
)
PY
    done
done
