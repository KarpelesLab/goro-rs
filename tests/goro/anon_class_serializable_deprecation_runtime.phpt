--TEST--
Anonymous class with interfaces is linked when new class runs: Serializable deprecation then
--FILE--
<?php
echo "start\n";
set_error_handler(function ($n, $s, $f, $l) { echo "handler: $s @ $l\n"; return true; });
function f() {
    $o = new class implements Serializable {
        public function serialize(): void {}
        public function unserialize(string $data): void {}
    };
    echo "made\n";
}
echo "before\n";
f();
f();
interface I {}
trait T {}
function g() {
    $o = new class implements I { use T; };
    echo strstr(get_class($o), "\0", true), "\n";
}
g();

?>
--EXPECT--
start
before
handler: Serializable@anonymous implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary) @ 5
made
made
I@anonymous
