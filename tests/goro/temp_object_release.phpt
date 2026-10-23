--TEST--
Temporary objects consumed by echo, concatenation and == are released immediately
--FILE--
<?php
// Temporary objects consumed by echo, concatenation and == are destroyed
// right away (and their handles reused)
class Str { public $n; function __construct($n) { $this->n = $n; } public function __toString(): string { return "str{$this->n}"; } function __destruct() { echo "[dtor {$this->n}]"; } }
echo new Str(1), "\n";
echo "after1\n";
echo "x" . new Str(2) . "\n";
echo "after2\n";
var_dump(new stdClass);
function f() { echo new Str(3), "\n"; echo "in f\n"; }
f();
var_dump(new Str(4) == "str4");
echo "after4\n";
var_dump(new stdClass);
--EXPECT--
str1[dtor 1]
after1
[dtor 2]xstr2
after2
object(stdClass)#1 (0) {
}
str3[dtor 3]
in f
[dtor 4]bool(true)
after4
object(stdClass)#1 (0) {
}
