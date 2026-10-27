--TEST--
Generator->throw(): yield from values released at the yield from, the frame's variables freed at once
--FILE--
<?php
function prime(Generator $generator) { $generator->valid(); }
$g = (function ()
{
    $x = 1;

    yield from [null, new class {
		function __destruct() {
			echo (new Exception)->getTraceAsString(), "\n";
		}
	}];
})();
prime($g);
try { $g->throw(new Error("e")); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$h = (function ()
{
    $x = 1;
    $obj = new class {
		function __destruct() {
			echo (new Exception)->getTraceAsString(), "\n";
		}
	};
    yield from [null, $obj];
})();
prime($h);
try { $h->throw(new Error("e2")); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
#0 %s(7): class@anonymous->__destruct()
#1 [internal function]: {closure:%s:3}()
#2 %s(14): Generator->throw(Object(Error))
#3 {main}
e
#0 [internal function]: class@anonymous->__destruct()
#1 %s(26): Generator->throw(Object(Error))
#2 {main}
e2
