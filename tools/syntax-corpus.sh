#!/bin/bash
# Parses the --FILE-- section of every php-src .phpt test with goro-syntax
# and compares with the expected output (parse error or not).
#
# Usage: tools/syntax-corpus.sh [TESTS_DIR] [--report FILE] [--bench]
# Default TESTS_DIR: /tmp/claude-1000/php-tests
set -e
cd "$(dirname "$0")/.."
exec cargo run --release -q -p goro-syntax --example corpus -- "$@"
