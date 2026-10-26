--TEST--
An Exception subclass may list Throwable
--FILE--
<?php
class E extends Exception implements Throwable {} echo "ok\n";
--EXPECT--
ok
