#!/bin/bash
# Run PHP test suite against goro-rs
# Usage: ./run_tests.sh [test_dir] [max_tests]

GORO="./target/release/goro"
TEST_DIR="${1:-/tmp/php-8.5-tests/Zend/tests}"
MAX_TESTS="${2:-0}"

PASS=0
FAIL=0
SKIP=0
ERROR=0
TOTAL=0

declare -A FAIL_REASONS

run_one_test() {
    local phpt_file="$1"
    local test_name=""
    local file_section=""
    local expect_section=""
    local expectf_section=""
    local skipif_section=""
    local ini_section=""
    local current_section=""
    local has_expect=0
    local has_expectf=0
    local has_skipif=0

    # Parse PHPT file
    while IFS= read -r line; do
        if [[ "$line" =~ ^--([A-Z_]+)--$ ]]; then
            current_section="${BASH_REMATCH[1]}"
            continue
        fi
        case "$current_section" in
            TEST) test_name+="$line" ;;
            FILE) file_section+="$line"$'\n' ;;
            EXPECT) expect_section+="$line"$'\n'; has_expect=1 ;;
            EXPECTF) expectf_section+="$line"$'\n'; has_expectf=1 ;;
            SKIPIF) skipif_section+="$line"$'\n'; has_skipif=1 ;;
            INI) ini_section+="$line"$'\n' ;;
        esac
    done < "$phpt_file"

    if [[ -z "$file_section" ]]; then
        echo "ERROR: $test_name (no FILE section)"
        ((ERROR++))
        return
    fi

    if [[ $has_expect -eq 0 && $has_expectf -eq 0 ]]; then
        echo "SKIP: $test_name (no EXPECT/EXPECTF)"
        ((SKIP++))
        return
    fi

    # Write file section to temp file
    local tmpfile=$(mktemp /tmp/goro_test_XXXXXX.php)
    echo -n "$file_section" > "$tmpfile"

    # Run goro
    local actual
    actual=$($GORO "$tmpfile" 2>&1)
    local exit_code=$?

    rm -f "$tmpfile"

    # Compare output
    if [[ $has_expect -eq 1 ]]; then
        # Remove trailing newline from expected
        local expected="${expect_section%$'\n'}"
        local actual_trimmed="${actual%$'\n'}"
        if [[ "$actual_trimmed" == "$expected" ]]; then
            ((PASS++))
            return
        fi
    fi

    if [[ $has_expectf -eq 1 ]]; then
        # Simple EXPECTF: for now just do exact match (TODO: pattern matching)
        local expected="${expectf_section%$'\n'}"
        local actual_trimmed="${actual%$'\n'}"
        if [[ "$actual_trimmed" == "$expected" ]]; then
            ((PASS++))
            return
        fi
    fi

    ((FAIL++))
    # Categorize failure
    if [[ "$actual" == *"Parse error"* ]]; then
        FAIL_REASONS["parse_error"]=$(( ${FAIL_REASONS["parse_error"]:-0} + 1 ))
    elif [[ "$actual" == *"Compile error"* ]]; then
        FAIL_REASONS["compile_error"]=$(( ${FAIL_REASONS["compile_error"]:-0} + 1 ))
    elif [[ "$actual" == *"Call to undefined function"* ]]; then
        FAIL_REASONS["undefined_function"]=$(( ${FAIL_REASONS["undefined_function"]:-0} + 1 ))
    elif [[ "$actual" == *"Fatal error"* ]]; then
        FAIL_REASONS["fatal_error"]=$(( ${FAIL_REASONS["fatal_error"]:-0} + 1 ))
    elif [[ "$actual" == *"panicked"* ]]; then
        FAIL_REASONS["panic"]=$(( ${FAIL_REASONS["panic"]:-0} + 1 ))
    elif [[ "$actual" == *"unimplemented"* ]]; then
        FAIL_REASONS["unimplemented"]=$(( ${FAIL_REASONS["unimplemented"]:-0} + 1 ))
    else
        FAIL_REASONS["wrong_output"]=$(( ${FAIL_REASONS["wrong_output"]:-0} + 1 ))
    fi

    # Print first few failures
    if [[ $FAIL -le 5 ]]; then
        echo "FAIL: $test_name"
        echo "  FILE: $phpt_file"
        echo "  GOT: ${actual:0:200}"
    fi
}

echo "Running tests from: $TEST_DIR"
echo "================================"

for f in $(find "$TEST_DIR" -name "*.phpt" -maxdepth 1 | sort); do
    ((TOTAL++))
    if [[ $MAX_TESTS -gt 0 && $TOTAL -gt $MAX_TESTS ]]; then
        break
    fi
    run_one_test "$f"
done

echo ""
echo "================================"
echo "Results: $PASS pass, $FAIL fail, $SKIP skip, $ERROR error (total: $TOTAL)"
echo ""
echo "Failure breakdown:"
for reason in "${!FAIL_REASONS[@]}"; do
    echo "  $reason: ${FAIL_REASONS[$reason]}"
done | sort -t: -k2 -rn
