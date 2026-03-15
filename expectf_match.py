#!/usr/bin/env python3
"""EXPECTF pattern matching for PHP test suite."""
import sys
import re

def expectf_to_regex(pattern):
    """Convert EXPECTF pattern to regex."""
    result = []
    i = 0
    while i < len(pattern):
        if pattern[i] == '%' and i + 1 < len(pattern):
            c = pattern[i + 1]
            if c == 'd':
                result.append(r'-?\d+')
                i += 2
            elif c == 'i':
                result.append(r'[+-]?\d+')
                i += 2
            elif c == 's':
                result.append(r'\S+')
                i += 2
            elif c == 'S':
                result.append(r'.*')
                i += 2
            elif c in ('a', 'A'):
                result.append(r'[\s\S]*')
                i += 2
            elif c == 'w':
                result.append(r'\s*')
                i += 2
            elif c == 'f':
                result.append(r'-?\d*\.?\d+(?:[eE][+-]?\d+)?')
                i += 2
            elif c == 'c':
                result.append(r'.')
                i += 2
            elif c == 'x':
                result.append(r'[0-9a-fA-F]+')
                i += 2
            elif c == 'e':
                result.append(r'[/\\]')
                i += 2
            elif c == '%':
                result.append(r'%')
                i += 2
            else:
                result.append(re.escape(pattern[i]))
                i += 1
        else:
            result.append(re.escape(pattern[i]))
            i += 1
    return ''.join(result)

def match_expectf(pattern, actual):
    """Check if actual output matches EXPECTF pattern."""
    regex = expectf_to_regex(pattern.rstrip('\n'))
    try:
        return bool(re.fullmatch(regex, actual.rstrip('\n'), re.DOTALL))
    except re.error:
        return False

if __name__ == '__main__':
    if len(sys.argv) != 3:
        print("Usage: expectf_match.py <pattern_file> <actual_file>", file=sys.stderr)
        sys.exit(2)

    with open(sys.argv[1]) as f:
        pattern = f.read()
    with open(sys.argv[2]) as f:
        actual = f.read()

    if match_expectf(pattern, actual):
        sys.exit(0)  # match
    else:
        sys.exit(1)  # no match
