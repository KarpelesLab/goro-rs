--TEST--
Unqualified calls of builtins in a namespace pass their by-reference arguments
--FILE--
<?php
namespace Foo\Bar;

class A {
    static function f($url) {
        if (!preg_match('/^http(s)?:\/\//i', $url, $matches)) {
            return null;
        }
        return $matches;
    }
}
var_dump(A::f('https://example.com/'));
preg_match_all('/\d/', 'a1b2', $all);
var_dump($all);
parse_str('a=1&b[]=2', $out);
var_dump($out);
str_replace('a', 'b', 'aaa', $count);
var_dump($count);
function local_fn(&$x) { $x = 'set'; }
local_fn($y);
var_dump($y);
?>
--EXPECT--
array(2) {
  [0]=>
  string(8) "https://"
  [1]=>
  string(1) "s"
}
array(1) {
  [0]=>
  array(2) {
    [0]=>
    string(1) "1"
    [1]=>
    string(1) "2"
  }
}
array(2) {
  ["a"]=>
  string(1) "1"
  ["b"]=>
  array(1) {
    [0]=>
    string(1) "2"
  }
}
int(3)
string(3) "set"
