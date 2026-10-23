--TEST--
strcmp()/strncmp()/substr_compare() return the first differing bytes' difference
--FILE--
<?php
// strcmp()/strncmp()/substr_compare() return memcmp()'s byte difference
var_dump(strcmp('  pad  ', 'aBc'), strcmp('abc', 'abd'), strcmp('b', 'a'), strcmp('abc', 'ab'), strcmp('ab', 'abc'), strcmp('', ''), strcmp("\xff", "\x01"));
var_dump(strncmp('<b>t', '123', 2), strncmp('abc', 'x', 10), strncmp('abcd', 'abcz', 3), strncmp('ab', 'abc', 5));
var_dump(substr_compare('Hello World', '%s-%d', 10, 10), substr_compare('aBc', 'abc', 1, 2, false), substr_compare('aBc', 'ABD', 0, 3, true), substr_compare('abcde', 'bc', 1, 3));
var_dump(strcasecmp('Hello', 'hellO'), strcasecmp('a', 'C'), strncasecmp('ab', 'AC', 2));
--EXPECT--
int(-65)
int(-1)
int(1)
int(1)
int(-1)
int(0)
int(254)
int(11)
int(-23)
int(0)
int(-1)
int(63)
int(-31)
int(-1)
int(1)
int(0)
int(-2)
int(-1)
