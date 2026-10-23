--TEST--
++/-- of a property read through &__get writes the copy back even when incrementing it throws; ++$o->p reports the line of its property
--FILE--
<?php
try {
    var_dump((function() { return new class {
        public $bar;
        function __construct() { $this->bar = new stdClass; }
        function &__get($x) { echo "get\n"; return $this->bar; }
        function __destruct() { echo "dtor\n"; }
    }; })()->foo++);
} catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try {
    var_dump(++(function() { return new class {
        public $bar;
        function __construct() { $this->bar = new stdClass; }
        function &__get($x) { echo "get\n"; return $this->bar; }
        function __destruct() { echo "dtor\n"; }
    }; })()->foo);
} catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$o = new class {
        public $bar;
        function __construct() { $this->bar = 5; }
        function &__get($x) { echo "get\n"; return $this->bar; }
};
var_dump($o->foo++);
var_dump($o);
?>
--EXPECTF--
get

Deprecated: Creation of dynamic property class@anonymous::$foo is deprecated in %s on line 8
dtor
TypeError: Cannot increment stdClass
get

Deprecated: Creation of dynamic property class@anonymous::$foo is deprecated in %s on line 16
dtor
TypeError: Cannot increment stdClass
get

Deprecated: Creation of dynamic property class@anonymous::$foo is deprecated in %s on line 23
int(5)
object(class@anonymous)#1 (2) {
  ["bar"]=>
  int(5)
  ["foo"]=>
  int(6)
}
