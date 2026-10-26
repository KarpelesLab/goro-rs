--TEST--
Exception subclasses may implement Throwable and interfaces extending it
--FILE--
<?php
interface I extends Throwable {} class E extends Exception implements I {} class F extends TypeError implements I {} echo "ok\n";
--EXPECT--
ok
