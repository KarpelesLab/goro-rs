#!/bin/bash
# Show random test failures from the latest CI run or local test
# Usage: ./tools/show_failures.sh [count]
#
# Pulls the test-results artifact from the latest GitHub Actions run.
# Falls back to local test run if no CI results available.

set -e

COUNT=${1:-50}

fetch_from_ci() {
    local run_id
    run_id=$(gh run list --workflow=test.yml --status=completed --limit=1 --json databaseId -q '.[0].databaseId' 2>/dev/null)
    if [ -z "$run_id" ]; then
        return 1
    fi

    local tmpdir
    tmpdir=$(mktemp -d)
    if ! gh run download "$run_id" --name test-results --dir "$tmpdir" 2>/dev/null; then
        rm -rf "$tmpdir"
        return 1
    fi

    if [ ! -f "$tmpdir/test_results.txt" ]; then
        rm -rf "$tmpdir"
        return 1
    fi

    echo "$tmpdir/test_results.txt"
}

run_local() {
    local php_src="${PHP_SRC:-/home/magicaltux/php-src}"
    if [ ! -d "$php_src" ]; then
        php_src="/tmp/php-src"
        if [ ! -d "$php_src" ]; then
            echo "Cloning PHP test suite..." >&2
            git clone --depth 1 --branch php-8.5.4 https://github.com/php/php-src.git "$php_src" >&2
        fi
    fi

    if [ ! -f "./target/release/goro" ]; then
        echo "Building..." >&2
        cargo build --release 2>&1 | tail -3 >&2
    fi

    ./target/release/goro --test "$php_src/" 2>&1 | tee /tmp/test_results.txt >&2
    echo "/tmp/test_results.txt"
}

# Try CI first, fall back to local
echo "Fetching latest CI results..."
RESULTS_FILE=$(fetch_from_ci) || {
    echo "No CI results available. Running tests locally..."
    RESULTS_FILE=$(run_local)
}

echo ""
echo "=== Test Summary ==="
grep -E "^(Pass|Fail|Skip|Error|Total):" "$RESULTS_FILE"
echo ""

TOTAL_FAILS=$(grep -c "^FAIL:" "$RESULTS_FILE" || echo 0)
echo "Showing $COUNT random failures out of $TOTAL_FAILS total:"
echo "================================================================"
echo ""

# Parse failures and show random sample
python3 -c "
import random

with open('$RESULTS_FILE') as f:
    lines = f.readlines()

blocks = []
i = 0
while i < len(lines):
    if lines[i].startswith('FAIL:'):
        name = lines[i].strip()[6:]
        expected = lines[i+1].strip().split('Expected: ', 1)[-1][:200] if i+1 < len(lines) and 'Expected:' in lines[i+1] else ''
        actual = lines[i+2].strip().split('Actual:   ', 1)[-1][:200] if i+2 < len(lines) and 'Actual:' in lines[i+2] else ''
        blocks.append((name, expected, actual))
        i += 3
    else:
        i += 1

for name, expected, actual in random.sample(blocks, min($COUNT, len(blocks))):
    print(f'FAIL: {name}')
    if expected:
        print(f'  Expected: {expected}')
    if actual:
        print(f'  Actual:   {actual}')
    print()
"
