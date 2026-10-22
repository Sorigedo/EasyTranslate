#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .build
xcrun swiftc native/AXFixture.swift -o .build/AXFixture
cargo build --example ax_fixture_check
.build/AXFixture &
fixture_pid=$!
trap 'kill "$fixture_pid" 2>/dev/null || true' EXIT
sleep 1
target/debug/examples/ax_fixture_check "$fixture_pid"
