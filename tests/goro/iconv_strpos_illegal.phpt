--TEST--
iconv_strpos()/iconv_strrpos(): an illegal sequence after the match
--FILE--
<?php
// glibc reports a bad sequence with the character before it
foreach (["ab\xff", "abc\xffdef", "\xffabc", "a\xe3\x81"] as $h) {
    foreach (["a", "b", "c", "x"] as $n) {
        echo bin2hex($h), " $n:\n";
        var_dump(iconv_strpos($h, $n), iconv_strrpos($h, $n), iconv_strpos($h, $n, 1));
    }
}
--EXPECTF--
6162ff a:

Notice: iconv_strrpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6
int(0)
bool(false)
bool(false)
6162ff b:

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strrpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6
bool(false)
bool(false)
bool(false)
6162ff c:

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strrpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6
bool(false)
bool(false)
bool(false)
6162ff x:

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strrpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6
bool(false)
bool(false)
bool(false)
616263ff646566 a:

Notice: iconv_strrpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6
int(0)
bool(false)
bool(false)
616263ff646566 b:

Notice: iconv_strrpos(): Detected an illegal character in input string in %s on line 6
int(1)
bool(false)
int(1)
616263ff646566 c:

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strrpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6
bool(false)
bool(false)
bool(false)
616263ff646566 x:

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strrpos(): Detected an illegal character in input string in %s on line 6

Notice: iconv_strpos(): Detected an illegal character in input string in %s on line 6
bool(false)
bool(false)
bool(false)
ff616263 a:

Fatal error: Uncaught ValueError: iconv_strpos(): Argument #3 ($offset) must be contained in argument #1 ($haystack) in %s:6
Stack trace:
#0 %s(6): iconv_strpos('\xFFabc', 'a', 1)
#1 {main}
  thrown in %s on line 6
