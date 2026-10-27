--TEST--
A deprecated constant used in a #[\Deprecated] message: reported at the use
--FILE--
<?php
#[\Deprecated]
const MSG = "use B";

#[\Deprecated(MSG)]
function f() {}

#[\Deprecated(
    MSG
)]
const C = 1;

f();
echo C, "\n";
--EXPECTF--

Deprecated: Constant MSG is deprecated in %s on line 13

Deprecated: Function f() is deprecated, use B in %s on line 13

Deprecated: Constant MSG is deprecated in %s on line 14

Deprecated: Constant C is deprecated, use B in %s on line 14
1
