--TEST--
preg_replace_callback() subjects after the callback threw
--FILE--
<?php
// With the callback's exception pending, the remaining subjects are still
// matched (each one resets preg_last_error()); a match fails.
foreach ([['/a/', 'ab'], ['/a/', ['ab']], ['/a/', ['ab', 'cd']], ['/a/', ['zz', 'ab']]] as [$p, $s]) {
    try { preg_replace_callback($p, fn($m) => throw new Exception("x"), $s); } catch (Exception $e) { echo $e->getMessage(), " "; }
    echo preg_last_error_msg(), "\n";
}
try { preg_replace_callback_array(["/a/" => fn($m) => throw new Exception("y")], ["ab", "cd"]); } catch (Exception $e) { echo $e->getMessage(), " "; }
echo preg_last_error_msg(), "\n";
--EXPECT--
x Internal error
x Internal error
x No error
x Internal error
y No error
