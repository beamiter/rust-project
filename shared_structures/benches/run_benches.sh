#!/usr/bin/env bash
set -euo pipefail

# 可选：清空上次结果（不清也可，Criterion 会在 data 目录内叠加 baselines）
rm -rf target/criterion

echo "Running benches with use-futex as baseline..."
cargo bench --features use-futex --bench ring_buffer_bench -- --save-baseline futex

echo "Running benches with use-eventfd, comparing to futex baseline..."
cargo bench --features use-eventfd --bench ring_buffer_bench -- --baseline futex
# cargo bench --features use-eventfd --bench ring_buffer_bench -- --save-baseline eventfd

echo "Running benches with use-semaphore, comparing to futex baseline..."
cargo bench --features use-semaphore --bench ring_buffer_bench -- --baseline futex
# cargo bench --features use-semaphore --bench ring_buffer_bench -- --save-baseline semaphore

echo "Done. Open target/criterion/report/index.html to view comparisons."
