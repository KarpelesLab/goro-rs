#!/bin/bash
# Run PHP test suite against goro-rs with EXPECTF support
# Usage: ./run_tests_v2.sh [test_dir] [max_tests]

GORO="./target/release/goro"
TEST_DIR="${1:-/tmp/php-8.5-tests/Zend/tests}"
MAX_TESTS="${2:-0}"

PASS=0
FAIL=0
SKIP=0
TOTAL=0

# Convert EXPECTF pattern to regex
expectf_to_regex() {
    local pattern="$1"
    # Escape regex special chars first
    pattern=$(echo "$pattern" | sed \
        -e 's/\\/\\\\/g' \
        -e 's/\./\\./g' \
        -e 's/\*/\\*/g' \
        -e 's/\+/\\+/g' \
        -e 's/\?/\\?/g' \
        -e 's/\[/\\[/g' \
        -e 's/\]/\\]/g' \
        -e 's/\^/\\^/g' \
        -e 's/\$/\\$/g' \
        -e 's/|/\\|/g' \
        -e 's/(/\\(/g' \
        -e 's/)/\\)/g' \
        -e 's/{/\\{/g' \
        -e 's/}/\\}/g' \
    )
    # Now convert EXPECTF patterns
    pattern=$(echo "$pattern" | sed \
        -e 's/%d/-\\?[0-9]\\+/g' \
        -e 's/%i/[+-]\\?[0-9]\\+/g' \
        -e 's/%f/-\\?[0-9]*\\.\\?[0-9]\\+/g' \
        -e 's/%s/[^ ]\\+/g' \
        -e 's/%S/.*/g' \
        -e 's/%a/.*/g' \
        -e 's/%A/.*/g' \
        -e 's/%w/ */g' \
        -e 's/%c/./g' \
        -e 's/%x/[0-9a-fA-F]\\+/g' \
        -e 's|%e|[/\\\\]|g' \
    )
    echo "$pattern"
}

for f in $(find "$TEST_DIR" -name "*.phpt" -maxdepth 1 | sort); do
    TOTAL=$((TOTAL + 1))
    if [ $MAX_TESTS -gt 0 ] && [ $TOTAL -gt $MAX_TESTS ]; then break; fi

    tmpfile=$(mktemp /tmp/goro_XXXXXX.php)
    sed -n '/^--FILE--$/,/^--[A-Z_]*--$/{ /^--/d; p }' "$f" > "$tmpfile"
    if [ ! -s "$tmpfile" ]; then rm -f "$tmpfile"; SKIP=$((SKIP + 1)); continue; fi

    # Get expected output
    has_expect=0; has_expectf=0
    expected=$(sed -n '/^--EXPECT--$/,/^--[A-Z_]*--$/{ /^--/d; p }' "$f")
    if [ -n "$expected" ]; then has_expect=1; fi
    expectf=$(sed -n '/^--EXPECTF--$/,/^--[A-Z_]*--$/{ /^--/d; p }' "$f")
    if [ -n "$expectf" ]; then has_expectf=1; fi

    if [ $has_expect -eq 0 ] && [ $has_expectf -eq 0 ]; then
        rm -f "$tmpfile"; SKIP=$((SKIP + 1)); continue
    fi

    result=$(timeout 5 "$GORO" "$tmpfile" 2>&1)
    rm -f "$tmpfile"

    r="${result%$'\n'}"

    if [ $has_expect -eq 1 ]; then
        e="${expected%$'\n'}"
        if [ "$r" = "$e" ]; then
            PASS=$((PASS + 1)); continue
        fi
    fi

    if [ $has_expectf -eq 1 ]; then
        e="${expectf%$'\n'}"
        # Convert EXPECTF to regex and try matching
        regex=$(expectf_to_regex "$e")
        if echo "$r" | grep -qzP "^${regex}$" 2>/dev/null; then
            PASS=$((PASS + 1)); continue
        fi
        # Fallback: exact match
        if [ "$r" = "$e" ]; then
            PASS=$((PASS + 1)); continue
        fi
    fi

    FAIL=$((FAIL + 1))
done

echo "Results: $PASS pass, $FAIL fail, $SKIP skip (total: $TOTAL)"
