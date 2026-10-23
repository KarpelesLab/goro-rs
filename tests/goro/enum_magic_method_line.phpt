--TEST--
A forbidden magic method of an enum is reported at the enum's line
--FILE--
<?php
enum E {
 public function __toString(): string { return ''; }
}
echo "ok\n";
--EXPECTF--

Fatal error: Enum E cannot include magic method __toString in %s on line 2
