--TEST--
A write to an unset() typed property with __set() passes the value to __set() without checking the property type
--FILE--
<?php
class C {
    public ?array $groups = null;
    public int $i;
    public int $never;
    public function __construct() { unset($this->groups, $this->i); }
    public function __set(string $o, mixed $v): void {
        echo "__set $o: ", get_debug_type($v), "\n";
        if ($o === 'groups') {
            $this->groups = (array) $v;
        } else {
            try {
                $this->$o = $v;
            } catch (TypeError $e) {
                echo $e->getMessage(), "\n";
            }
        }
    }
}
$f = new C();
$f->groups = 'x';
var_dump($f->groups);
$f->i = 'abc';
$f->i = 'def';
try {
    $f->never = 'abc';
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
$f->i = '42';
var_dump($f->i);
try {
    $f->i = 'ghi';
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
// no __set: the type is checked
class D {
    public int $i = 1;
}
$d = new D;
unset($d->i);
try {
    $d->i = 'abc';
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}
// inherited __set
abstract class Base {
    public ?array $groups = null;
    public function __construct(?array $groups = null) {
        unset($this->groups);
        if (null !== $groups) {
            $this->groups = $groups;
        }
    }
    public function __set(string $o, mixed $v): void { echo "Base::__set $o\n"; $this->groups = (array) $v; }
}
class Child extends Base {}
$c = new Child();
$c->groups = 'g';
var_dump($c->groups);
$c = new Child(['a']);
try {
    $c->groups = 'y';
} catch (TypeError $e) {
    echo $e->getMessage(), "\n";
}

?>
--EXPECT--
__set groups: string
array(1) {
  [0]=>
  string(1) "x"
}
__set i: string
Cannot assign string to property C::$i of type int
__set i: string
Cannot assign string to property C::$i of type int
Cannot assign string to property C::$never of type int
__set i: string
int(42)
Cannot assign string to property C::$i of type int
Cannot assign string to property D::$i of type int
Base::__set groups
array(1) {
  [0]=>
  string(1) "g"
}
Base::__set groups
Cannot assign string to property Base::$groups of type ?array
