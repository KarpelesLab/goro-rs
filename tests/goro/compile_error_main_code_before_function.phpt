--TEST--
A compile error in the main code comes before one in a later function body
--FILE--
<?php
$x = 1;
break;
function f() {
  break;
}
--EXPECTF--

Fatal error: 'break' not in the 'loop' or 'switch' context in %s on line 3
