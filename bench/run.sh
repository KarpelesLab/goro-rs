#!/bin/sh
# Runs the benchmarks in this directory one after another and prints a table.
#
#   bench/run.sh [-g GORO] [-p PHP] [-n RUNS] [NAME...]
#
#   -g GORO  goro binary (default: target/release/goro, or $GORO)
#   -p PHP   reference PHP binary to compare against (optional, or $PHP)
#   -n RUNS  runs per benchmark; the best time is kept (default 3)
#   NAME     benchmark names (file names without .php); default: all
#
# Each script times its own work with hrtime() and prints
# "time_ms=<ms> check=<value>", so interpreter startup is not counted. The
# check column says whether goro and PHP computed the same result.
#
# goro runs under a 4 GB address-space cap and a 120 s timeout.

set -u
dir=$(cd "$(dirname "$0")" && pwd)
goro=${GORO:-$dir/../target/release/goro}
php=${PHP:-}
runs=3
while getopts g:p:n: opt; do
    case $opt in
        g) goro=$OPTARG ;;
        p) php=$OPTARG ;;
        n) runs=$OPTARG ;;
        *) exit 2 ;;
    esac
done
shift $((OPTIND - 1))

if [ $# -eq 0 ]; then
    set -- $(cd "$dir" && ls *.php | sed 's/\.php$//')
fi

# best_of CMD...: runs CMD $runs times, prints "<best ms> <check>".
best_of() {
    best=""
    check=""
    i=0
    while [ $i -lt "$runs" ]; do
        out=$("$@" 2>&1 | grep '^time_ms=' | tail -n 1)
        ms=$(printf '%s\n' "$out" | sed -n 's/^time_ms=\([0-9.]*\).*/\1/p')
        check=$(printf '%s\n' "$out" | sed -n 's/.* check=//p')
        if [ -n "$ms" ]; then
            if [ -z "$best" ] || awk -v a="$ms" -v b="$best" 'BEGIN { exit !(a < b) }'; then
                best=$ms
            fi
        fi
        i=$((i + 1))
    done
    printf '%s %s\n' "${best:-FAIL}" "${check:-?}"
}

if [ -n "$php" ]; then
    printf '%-12s %10s %10s %7s  %s\n' benchmark goro_ms php_ms ratio check
else
    printf '%-12s %10s  %s\n' benchmark goro_ms check
fi
for name in "$@"; do
    script=$dir/$name.php
    set -- $(best_of prlimit --as=4294967296 timeout 120 "$goro" "$script")
    g_ms=$1
    g_check=$2
    if [ -n "$php" ]; then
        set -- $(best_of timeout 300 "$php" -n -d memory_limit=-1 "$script")
        p_ms=$1
        p_check=$2
        ratio=$(awk -v g="$g_ms" -v p="$p_ms" 'BEGIN { if (p + 0 > 0 && g + 0 > 0) printf "%.2f", g / p; else print "-" }')
        same=ok
        [ "$g_check" = "$p_check" ] || same="DIFF (goro $g_check, php $p_check)"
        printf '%-12s %10s %10s %7s  %s\n' "$name" "$g_ms" "$p_ms" "$ratio" "$same"
    else
        printf '%-12s %10s  %s\n' "$name" "$g_ms" "$g_check"
    fi
done
