--TEST--
exit(): strict_types argument checks; exit as an array_map() callback stops the script
--FILE--
<?php
declare(strict_types=1);
try { exit(1.0); } catch (TypeError $t) { echo $t->getMessage(), "\n"; }
try { exit(true); } catch (TypeError $t) { echo $t->getMessage(), "\n"; }
try { exit(null); } catch (TypeError $t) { echo $t->getMessage(), "\n"; }
try { exit([]); } catch (TypeError $t) { echo $t->getMessage(), "\n"; }
$f = 'exit';
try { $f(2.5); } catch (TypeError $t) { echo $t->getMessage(), "\n"; }
echo "after\n";
array_map('exit', ["one\n", "two\n"]);
echo "not reached\n";
?>
--EXPECT--
exit(): Argument #1 ($status) must be of type string|int, float given
exit(): Argument #1 ($status) must be of type string|int, true given
exit(): Argument #1 ($status) must be of type string|int, null given
exit(): Argument #1 ($status) must be of type string|int, array given
exit(): Argument #1 ($status) must be of type string|int, float given
after
one
