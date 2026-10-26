--TEST--
str_replace, strpos, str_contains, trim, in_array and preg_* results (substring search, character masks, strict search, regex cache)
--FILE--
<?php
function show($v) { echo json_encode($v), "\n"; }
// str_replace: leftmost, non-overlapping occurrences
show(str_replace("aa", "b", "aaaaa", $n)); show($n);
show(str_replace("aba", "X", "abababa", $n)); show($n);
show(str_replace("", "X", "abc", $n)); show($n);
show(str_replace("abcd", "X", "abc", $n)); show($n);
show(str_replace("abc", "X", "abc", $n)); show($n);
show(str_replace("x", "", "xaxbx", $n)); show($n);
show(str_replace("no", "yes", "nothing here, no", $n)); show($n);
show(str_replace("zz", "yy", "nothing here", $n)); show($n);
show(str_replace(["a", "b"], ["b", "c"], "ab", $n)); show($n);
show(str_replace(["a", "b", ""], "-", "abcab", $n)); show($n);
show(str_replace(["a", "bb"], ["1"], "abba", $n)); show($n);
show(str_replace("a", "b", ["k" => "aaa", 5 => "bab", "x" => ""], $n)); show($n);
show(str_replace(1, 2, 12131, $n)); show($n);
show(str_ireplace("AB", "x", "abAbaBAB", $n)); show($n);
show(str_ireplace(["A", "b"], ["1", "2"], "aBcAb", $n)); show($n);
$long = str_repeat("lorem ipsum dolor ", 50) . "needle" . str_repeat(" sit amet", 30);
show(strlen(str_replace("ipsum", "IPSUM", $long, $n))); show($n);
show(str_replace("\0", "0", "a\0b\0", $n)); show($n);
// strpos / str_contains
show(strpos("abcabc", "c")); show(strpos("abcabc", "c", 3)); show(strpos("abcabc", "c", -1));
show(strpos("abcabc", "c", -2)); show(strpos("abcabc", "bc", 5)); show(strpos("abcabc", "", 2));
show(strpos("abc", "abcd")); show(strpos($long, "needle")); show(strpos($long, "needles"));
show(strpos("aaab", "aab")); show(strpos("abcabc", "abc", 1)); show(strpos("a\0b", "\0"));
try { strpos("abc", "a", 4); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { strpos("abc", "a", -4); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
show(str_contains("abc", "")); show(str_contains("", "")); show(str_contains("abc", "bc"));
show(str_contains("abc", "cb")); show(str_contains($long, "needle")); show(str_contains("ab", "abc"));
// trim family: default list, given lists, ranges
show(trim("  \t\n\r\0\x0Babc \x0B\0")); show(ltrim("  abc  ")); show(rtrim("  abc  "));
show(trim("xxabcxx", "x")); show(trim("abcba", "a..b")); show(trim("[abc]", "[]"));
show(trim("", "x")); show(trim("xxxx", "x")); show(ltrim("xxxx", "x")); show(rtrim("xxxx", "x"));
show(trim("0012300", "0")); show(rtrim("1.5000", "0.")); show(ltrim("/path/", "/"));
show(trim("\xff\x80abc\x80", "\x80..\xff"));
show(trim("abc", "a..")); show(trim("abc", "..c")); show(trim("abc", "c..a")); show(trim("a..b", ".."));
show(trim(123, 1)); show(rtrim("abc\n"));
// in_array, strict and loose
$r = "two"; $ri = 2;
$arr = [1, "1", 2.0, "two", null, false, [1], &$r, &$ri];
foreach (["1", 1, "two", 2, 2.0, "2", null, false, [1], ["1"], "TWO", "", 0] as $needle) {
    echo json_encode($needle), ": ", json_encode(in_array($needle, $arr, true)), " ",
        json_encode(in_array($needle, $arr)), "\n";
}
show(in_array("a", [], true)); show(in_array(5, ["5"], true)); show(in_array("5", [5], true));
show(in_array("abc", ["x" => "abc"], true)); show(in_array(PHP_INT_MAX, [PHP_INT_MAX], true));
// preg_*: the regex cache and the pcre ini settings
for ($i = 0; $i < 3; $i++) {
    show(preg_match('/(\d+)-(\d+)/', "a 12-34 b", $m)); show($m);
    show(preg_replace('/o/', '0', "foo boo"));
}
show(preg_match('/(a+)+$/', str_repeat('a', 14) . 'X'));
show(preg_last_error_msg());
ini_set('pcre.jit', '0');
ini_set('pcre.backtrack_limit', '10');
show(preg_match('/(a+)+$/', str_repeat('a', 14) . 'X'));
show(preg_last_error_msg());
ini_set('pcre.backtrack_limit', ' 1000000');
show(preg_match('/(a+)+$/', str_repeat('a', 14) . 'X'));
show(preg_last_error_msg());
?>
--EXPECTF--
"bba"
2
"XbX"
2
"abc"
0
"abc"
0
"X"
1
"ab"
3
"yesthing here, yes"
2
"nothing here"
0
"cc"
3
"--c--"
4
"11"
3
{"k":"bbb","5":"bbb","x":""}
4
"22232"
3
"xxxx"
4
"12c12"
4
1176
50
"a0b0"
2
2
5
5
5
false
2
false
900
false
1
3
1
strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)
strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack)
true
true
true
false
true
false
"abc"
"abc  "
"  abc"
"abc"
"c"
"abc"
""
""
""
""
"123"
"1.5"
"path\/"
"abc"

Warning: trim(): Invalid '..'-range, no character to the right of '..' in %s on line 37
"bc"

Warning: trim(): Invalid '..'-range, no character to the left of '..' in %s on line 37
"ab"

Warning: trim(): Invalid '..'-range, '..'-range needs to be incrementing in %s on line 37
"b"

Warning: trim(): Invalid '..'-range, no character to the left of '..' in %s on line 37
"a..b"
"23"
"abc"
"1": true true
1: true true
"two": true true
2: true true
2: true true
"2": false true
null: true true
false: true true
[1]: true true
["1"]: false true
"TWO": false false
"": false true
0: false true
false
false
false
true
true
1
["12-34","12","34"]
"f00 b00"
1
["12-34","12","34"]
"f00 b00"
1
["12-34","12","34"]
"f00 b00"
0
"No error"
false
"Backtrack limit exhausted"
0
"No error"
