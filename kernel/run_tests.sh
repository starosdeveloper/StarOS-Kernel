#!/bin/bash
# Helper script to run tests on host system

cd "$(dirname "$0")"
cargo test --target x86_64-unknown-linux-gnu --features std --lib --tests "$@"
