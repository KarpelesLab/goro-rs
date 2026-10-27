--TEST--
A static call of an inaccessible method uses __call() only for a $this that is an instance of the class, else __callStatic()
--FILE--
<?php
class M { public function __call($m, $a) { return "__call ".get_class($this)." $m"; } public static function __callStatic($m, $a) { return "__callStatic $m"; } protected function isP() { return "isP"; } }
class M2 extends M {}
class C { function run($m) { return $m::isP(); } function run2() { return M::isP(); } function run3() { return M::nope(); } }
class D extends M { function run($m) { return [$m::isP(), M::nope()]; } }
var_dump((new C)->run("M"), (new C)->run("M2"), (new C)->run2(), (new C)->run3(), (new D)->run("M"));
?>
--EXPECT--
string(16) "__callStatic isP"
string(16) "__callStatic isP"
string(16) "__callStatic isP"
string(17) "__callStatic nope"
array(2) {
  [0]=>
  string(3) "isP"
  [1]=>
  string(13) "__call D nope"
}
