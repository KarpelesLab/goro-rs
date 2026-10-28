#!/usr/bin/env python3
"""Extract the cases of php-src's ext/date DateTime_{diff,days,add,sub}-*
tests (their DateTime_data-*.inc inputs and the --EXPECT-- lines) into a
fixture for goro-ext-date's tests. The tests are used as a black box: only
their inputs and expected output are read.

Usage: extract_math_tests.py PHP_TESTS_DIR/ext/date/tests OUT.txt

Output lines: kind TAB default_tz TAB end TAB start TAB spec TAB days TAB
absolute TAB expected. end/start are the constructor strings, with "@@zone" appended
when the test moves the value to another zone with setTimezone().
"""
import glob
import os
import re
import sys


def php_str(s):
    s = s.strip()
    m = re.match(r"^'(.*)'$", s) or re.match(r'^"(.*)"$', s)
    return m.group(1) if m else None


def cases_of(inc):
    tz = None
    out = []
    end = start = None
    for line in open(inc):
        m = re.search(r"date_default_timezone_set\('([^']+)'\)", line)
        if m:
            tz = m.group(1)
        m = re.match(r"\s*\$end\s*=\s*new DateTime\((.*?)\);", line)
        if m:
            end = php_str(m.group(1))
        m = re.match(r"\s*\$start\s*=\s*new DateTime\((.*?)\);", line)
        if m:
            start = php_str(m.group(1))
        m = re.match(r"\s*\$(end|start)->setTimezone\(new DateTimeZone\('([^']+)'\)\);", line)
        if m:
            if m.group(1) == "end":
                end += "@@" + m.group(2)
            else:
                start += "@@" + m.group(2)
        m = re.match(r"\s*examine_diff\((.*)\);", line)
        if m:
            args = [a.strip() for a in re.split(r",(?=(?:[^']*'[^']*')*[^']*$)", m.group(1))]
            e = php_str(args[0]) if args[0] != "$end" else end
            st = php_str(args[1]) if args[1] != "$start" else start
            spec = php_str(args[2])
            days = args[3]
            absolute = args[4] if len(args) > 4 else "false"
            out.append((tz, e, st, spec, days, absolute))
    return out


def main():
    d, outp = sys.argv[1], sys.argv[2]
    out = open(outp, "w")
    for kind in ["diff", "days", "add", "sub"]:
        for t in sorted(glob.glob(os.path.join(d, "DateTime_%s-*.phpt" % kind))):
            src = open(t).read()
            inc = re.search(r"require '(DateTime_data-[^']+\.inc)'", src)
            if not inc:
                continue
            cases = cases_of(os.path.join(d, inc.group(1)))
            parts = re.split(r"--EXPECTF?--\n", src, maxsplit=1)
            if len(parts) < 2:
                continue
            exp = parts[1].strip().split("\n")
            exp = [e for e in exp if e.startswith("test_")]
            if len(exp) != len(cases):
                print("skip", t, len(exp), len(cases))
                continue
            for c, e in zip(cases, exp):
                if c[1] is None or c[2] is None:
                    continue
                out.write("\t".join([kind] + [str(x) for x in c] + [e]) + "\n")


if __name__ == "__main__":
    main()
