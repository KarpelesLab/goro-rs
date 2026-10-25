--TEST--
return $undef in try/catch/finally: the error handler's exception is caught by the catch
--FILE--
<?php
set_error_handler(function($n, $s) { throw new Exception($s); });
function m1() {
    try { return $undef; } catch (Exception $e) { echo "caught m1\n"; return 1; } finally { echo "finally m1\n"; }
}
function m2() {
    try { $x = $undef; return 5; } catch (Exception $e) { echo "caught m2\n"; return 2; } finally { echo "finally m2\n"; }
}
function m3() {
    try { echo "a\n"; return $undef; } catch (Exception $e) { echo "caught m3\n"; return 3; } finally { echo "finally m3\n"; }
}
function m4() {
    try { try { return $undef; } finally { echo "finally m4\n"; } } catch (Exception $e) { echo "caught m4\n"; return 4; }
}
foreach (['m1','m2','m3','m4'] as $f) {
try { var_dump($f()); } catch (Exception $e) { echo "outer $f\n"; }
}
--EXPECT--
caught m1
finally m1
int(1)
caught m2
finally m2
int(2)
a
caught m3
finally m3
int(3)
finally m4
caught m4
int(4)
