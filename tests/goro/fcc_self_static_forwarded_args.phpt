--TEST--
self::m(...) of a static method forwarding the called scope binds its arguments
--FILE--
<?php
class C {
    private static function make(self $c, string $id, int $b = 9, &$r = null, ...$rest) { $r = "set"; return [static::class, $id, $b, $rest]; }
    public static function get() {
        $f = self::make(...);
        $x = null;
        var_dump($f(new static, 'a'));
        var_dump($f(new static, 'b', 2, $x, 5, 6), $x);
        var_dump($f(id: 'n', c: new static));
        $r = new ReflectionFunction($f);
        foreach ($r->getParameters() as $p) echo $p->getPosition(), ' ', $p->getName(), ' ', $p->isPassedByReference() ? 'ref' : 'val', ' ', $p->isOptional() ? 'opt' : 'req', ' ', $p->isVariadic() ? 'var' : '', "\n";
        var_dump($r->getNumberOfParameters(), $r->getNumberOfRequiredParameters());
        var_dump($f);
        try { $f('x'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
        try { $f(); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; }
    }
}
class D extends C {}
D::get();

?>
--EXPECTF--
array(4) {
  [0]=>
  string(1) "D"
  [1]=>
  string(1) "a"
  [2]=>
  int(9)
  [3]=>
  array(0) {
  }
}
array(4) {
  [0]=>
  string(1) "D"
  [1]=>
  string(1) "b"
  [2]=>
  int(2)
  [3]=>
  array(2) {
    [0]=>
    int(5)
    [1]=>
    int(6)
  }
}
string(3) "set"
array(4) {
  [0]=>
  string(1) "D"
  [1]=>
  string(1) "n"
  [2]=>
  int(9)
  [3]=>
  array(0) {
  }
}
0 c val req 
1 id val req 
2 b val opt 
3 r ref opt 
4 rest val opt var
int(5)
int(2)
object(Closure)#1 (2) {
  ["function"]=>
  string(7) "C::make"
  ["parameter"]=>
  array(5) {
    ["$c"]=>
    string(10) "<required>"
    ["$id"]=>
    string(10) "<required>"
    ["$b"]=>
    string(10) "<optional>"
    ["&$r"]=>
    string(10) "<optional>"
    ["$rest"]=>
    string(10) "<optional>"
  }
}
C::make(): Argument #1 ($c) must be of type C, string given, called in %s on line 14
Too few arguments to function C::make(), 0 passed in %s on line 15 and at least 2 expected
