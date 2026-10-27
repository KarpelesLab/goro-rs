--TEST--
A typed reference's self type names the property's declaring class, not the writer's scope
--FILE--
<?php
class Outer {
    public $m = 'x';
    function f() {
        $this->m = new class($this->m) {
            private string|self $message;
            public function __construct(&$message) { $this->message = &$message; }
            public function __toString(): string { return "s"; }
        };
        echo "assigned\n";
        $o = new class { public self $p; function set() { $this->p = $this; } };
        $o->set(); echo "ok\n";
        $r = new ReflectionProperty($o, 'p'); echo $r->getType(), "\n";
    }
}
(new Outer)->f();

?>
--EXPECT--
assigned
ok
self
