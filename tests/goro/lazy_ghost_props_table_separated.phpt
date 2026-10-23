--TEST--
Lazy ghost viewed while lazy, initializer adds a dynamic property: views keep the declared values it copied
--FILE--
<?php
#[AllowDynamicProperties]
class C {
    public int $b = 1;
    public $c = 5;
    public function __construct() {
        $this->a = 1;
        $this->b = 3;
    }
}
$reflector = new ReflectionClass(C::class);
$mk = fn() => $reflector->newLazyGhost(function ($obj) { $obj->__construct(); });

$obj = $mk(); var_dump($obj); $obj->b = 7; var_dump($obj->b); var_dump($obj); print_r((array)$obj); print_r(get_object_vars($obj));
$obj->b = 8; var_dump($obj);
$obj = $mk(); var_dump($obj); $obj->b = 7; $obj->c = 9; var_dump($obj);
$obj = $mk(); var_dump($obj); $obj->c = 9; var_dump($obj);
class D { public int $b = 1; public $c = 5; public function __construct() { $this->b = 3; } }
$reflector = new ReflectionClass(D::class);
$obj = $reflector->newLazyGhost(function ($obj) { $obj->__construct(); });
var_dump($obj); $obj->c = 9; var_dump($obj);
?>
--EXPECT--
lazy ghost object(C)#4 (0) {
  ["b"]=>
  uninitialized(int)
}
int(7)
object(C)#4 (3) {
  ["b"]=>
  int(1)
  ["c"]=>
  int(5)
  ["a"]=>
  int(1)
}
Array
(
    [b] => 1
    [c] => 5
    [a] => 1
)
Array
(
    [b] => 1
    [c] => 5
    [a] => 1
)
object(C)#4 (3) {
  ["b"]=>
  int(1)
  ["c"]=>
  int(5)
  ["a"]=>
  int(1)
}
lazy ghost object(C)#5 (0) {
  ["b"]=>
  uninitialized(int)
}
object(C)#5 (3) {
  ["b"]=>
  int(1)
  ["c"]=>
  int(5)
  ["a"]=>
  int(1)
}
lazy ghost object(C)#4 (0) {
  ["b"]=>
  uninitialized(int)
}
object(C)#4 (3) {
  ["b"]=>
  int(1)
  ["c"]=>
  int(5)
  ["a"]=>
  int(1)
}
lazy ghost object(D)#6 (0) {
  ["b"]=>
  uninitialized(int)
}
object(D)#6 (2) {
  ["b"]=>
  int(3)
  ["c"]=>
  int(9)
}
