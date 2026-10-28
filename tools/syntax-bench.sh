#!/bin/bash
# Lexer and parser throughput of goro-syntax on real PHP code.
#
# Usage:
#   tools/syntax-bench.sh [TESTS_DIR] [ROUNDS]     all php-src test sources
#   tools/syntax-bench.sh --file FILE [ROUNDS]     one file
#   tools/syntax-bench.sh --concat OUT             write one large file from
#                                                  the test sources
set -e
cd "$(dirname "$0")/.."
exec cargo run --release -q -p goro-syntax --example bench -- "$@"
