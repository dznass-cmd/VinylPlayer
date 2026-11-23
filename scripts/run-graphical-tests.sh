#!/usr/bin/env bash
set -euo pipefail

# GTK may only be initialized from one thread per process. Rust's test harness
# can assign sequential tests to different worker threads, so every ignored
# integration test gets a fresh process.
mapfile -t tests < <(
  cargo test --locked --all-targets -- --ignored --list \
    | sed -n 's/: test$//p'
)

if [[ ${#tests[@]} -eq 0 ]]; then
  echo "No ignored integration tests were discovered" >&2
  exit 1
fi

for test_name in "${tests[@]}"; do
  echo "Running $test_name"
  cargo test --locked --all-targets "$test_name" -- --ignored --exact --nocapture
done
