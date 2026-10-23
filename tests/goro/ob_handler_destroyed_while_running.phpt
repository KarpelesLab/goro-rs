--TEST--
An output handler object only the output layer holds is destroyed as the layer is deactivated, even while it runs
--FILE--
<?php
class Test {
    public function __destruct() { echo "dtor\n"; ob_start(); }
    public function __invoke($x) { ob_start("foo"); return $x; }
}
ob_start(new Test, 1);
echo "trigger";
?>
--EXPECTF--
dtor

Notice: ob_start(): Failed to create buffer in %s on line 3

Fatal error: ob_start(): Cannot use output buffering in output buffering display handlers in %s on line 4
