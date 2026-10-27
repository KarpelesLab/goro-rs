--TEST--
Builtins called with exact-type arguments (run without their frame): results, argument release, errors
--FILE--
<?php
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }
function mk($n) { return [new D($n)]; }
function g($s, $o, $arr) {
    $r = [];
    $r[] = strpos($s, "c"); $r[] = strpos($s, "zz"); $r[] = strpos($s, ""); $r[] = stripos($s, "C"); $r[] = strrpos($s, "c"); $r[] = strrpos($s, "");
    $r[] = trim("  $s \n"); $r[] = ltrim("\0\t$s "); $r[] = rtrim(" $s\x0B"); $r[] = ucfirst($s); $r[] = lcfirst(strtoupper($s));
    $r[] = str_repeat($s, 3); $r[] = str_repeat($s, 0); $r[] = str_repeat("", 5);
    $r[] = explode("c", $s); $r[] = explode("c", ""); $r[] = explode("abc", "abcabcx");
    $r[] = spl_object_id($o) > 0; $r[] = get_class($o); $r[] = get_class(new class {}) !== '';
    $r[] = array_key_first($arr); $r[] = array_key_last($arr); $r[] = array_key_first([]); $r[] = array_key_last(['x' => 1, 'y' => 2]);
    var_dump($r);
}
g("abcdcba", new ArrayObject([]), [3 => 'a', 5 => 'b']);
function h() {
    echo array_key_last(mk(1)), "\n";
    echo "after1\n";
    $x = array_key_first(mk(2));
    echo "after2\n";
    echo get_class(new D(3)), "\n";
    echo "after3\n";
    echo spl_object_id(new D(4)) > 0 ? "y" : "n", "\n";
    echo "after4\n";
    try { explode("", "a"); } catch (\ValueError $e) { echo $e->getMessage(), "\n"; }
    try { str_repeat("a", -1); } catch (\ValueError $e) { echo $e->getMessage(), "\n"; }
    var_dump(trim("xxaxx", "x"), strpos("abc", "c", 1), get_class(new D(5)));
}
h();
echo "end\n";
--EXPECT--
array(24) {
  [0]=>
  int(2)
  [1]=>
  bool(false)
  [2]=>
  int(0)
  [3]=>
  int(2)
  [4]=>
  int(4)
  [5]=>
  int(7)
  [6]=>
  string(7) "abcdcba"
  [7]=>
  string(8) "abcdcba "
  [8]=>
  string(8) " abcdcba"
  [9]=>
  string(7) "Abcdcba"
  [10]=>
  string(7) "aBCDCBA"
  [11]=>
  string(21) "abcdcbaabcdcbaabcdcba"
  [12]=>
  string(0) ""
  [13]=>
  string(0) ""
  [14]=>
  array(3) {
    [0]=>
    string(2) "ab"
    [1]=>
    string(1) "d"
    [2]=>
    string(2) "ba"
  }
  [15]=>
  array(1) {
    [0]=>
    string(0) ""
  }
  [16]=>
  array(3) {
    [0]=>
    string(0) ""
    [1]=>
    string(0) ""
    [2]=>
    string(1) "x"
  }
  [17]=>
  bool(true)
  [18]=>
  string(11) "ArrayObject"
  [19]=>
  bool(true)
  [20]=>
  int(3)
  [21]=>
  int(5)
  [22]=>
  NULL
  [23]=>
  string(1) "y"
}
dtor 1
0
after1
dtor 2
after2
dtor 3
D
after3
dtor 4
y
after4
explode(): Argument #1 ($separator) must not be empty
str_repeat(): Argument #2 ($times) must be greater than or equal to 0
dtor 5
string(1) "a"
int(2)
string(1) "D"
end
