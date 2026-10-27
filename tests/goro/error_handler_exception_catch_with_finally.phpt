--TEST--
An exception an error handler throws is caught by a try/catch that has a finally block
--FILE--
<?php
try { array_map(function () { throw new Exception("x"); }, [1]); } catch (Exception $e) { echo "caught1\n"; } finally { echo "fin\n"; }
try { intdiv(1, 0); } catch (Error $e) { echo "caught2\n"; } finally { echo "fin\n"; }
set_error_handler(function ($l, $m) { throw new ErrorException($m); });
try { strlen(); } catch (Error $e) { echo "caught3\n"; } finally { echo "fin\n"; }
try { echo $undef; } catch (Exception $e) { echo "caught4\n"; } finally { echo "fin\n"; }
try { str_repeat("x", -1); } catch (Throwable $e) { echo "caught5\n"; } finally { echo "fin\n"; }
try { hex2bin("a"); } catch (Throwable $e) { echo "caught6\n"; } finally { echo "fin\n"; }
try { trigger_error("x"); } catch (Throwable $e) { echo "caught7\n"; } finally { echo "fin\n"; }
function a() { trigger_error("dep", E_USER_DEPRECATED); }
function c() { $x = []; return $x["k"]; }
foreach (["a", "c"] as $f) {
    try { $f(); echo "no\n"; } catch (Throwable $e) { echo "caught $f\n"; } finally { echo "fin\n"; }
    try { try { $f(); } finally { echo "inner fin\n"; } } catch (Throwable $e) { echo "outer caught $f\n"; } finally { echo "outer fin\n"; }
}

?>
--EXPECT--
caught1
fin
caught2
fin
caught3
fin
caught4
fin
caught5
fin
caught6
fin
caught7
fin
caught a
fin
inner fin
outer caught a
outer fin
caught c
fin
inner fin
outer caught c
outer fin
