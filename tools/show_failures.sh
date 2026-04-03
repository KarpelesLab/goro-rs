#!/bin/bash
# Show 50 random test failures from the latest test run
# Usage: ./show_failures.sh [count] [directory]

COUNT=${1:-50}
DIR=${2:-/home/magicaltux/php-src/}

echo "Running test suite on $DIR..."
OUTPUT=$(./target/release/goro --test "$DIR" 2>&1)

# Extract summary
echo "$OUTPUT" | tail -6
echo ""

# Extract FAIL lines with their expected/actual
FAILS=$(echo "$OUTPUT" | grep -A 2 "^FAIL:" | paste - - - | grep "FAIL:")

TOTAL_FAILS=$(echo "$FAILS" | wc -l)
echo "Showing $COUNT random failures out of $TOTAL_FAILS total:"
echo "================================================================"
echo ""

echo "$FAILS" | shuf | head -n "$COUNT" | while IFS=$'\t' read -r fail_line exp_line act_line; do
    name=$(echo "$fail_line" | sed 's/^FAIL: //')
    expected=$(echo "$exp_line" | sed 's/^  Expected: //' | head -c 200)
    actual=$(echo "$act_line" | sed 's/^  Actual:   //' | head -c 200)
    
    echo "FAIL: $name"
    echo "  Expected: $expected"
    echo "  Actual:   $actual"
    echo ""
done
