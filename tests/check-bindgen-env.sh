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
