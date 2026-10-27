--TEST--
An autoloader unregistering itself makes the chain skip the next one; autoloader registration edge cases
--FILE--
<?php
spl_autoload_register(function($c) { echo "A $c\n"; spl_autoload_unregister(spl_autoload_functions()[0]); });
spl_autoload_register(function($c) { echo "B $c\n"; });
class_exists('X1'); class_exists('X2');
spl_autoload_register($f = function($c) { echo "C $c\n"; if ($c == 'Dyn') { eval('class Dyn {}'); } });
spl_autoload_register($f);
var_dump(count(spl_autoload_functions()));
var_dump(class_exists('Dyn'), class_exists('dyn'));
spl_autoload_call('Q');
try { spl_autoload_call(''); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(class_exists('\\Lead'));
var_dump(class_exists('bad name'));
var_dump(class_exists("x\0y"));
try { spl_autoload_register('nonexistent_function_xyz'); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { spl_autoload_register([new stdClass, 'nope']); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { spl_autoload_register('L::priv'); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(spl_autoload_register(fn($c) => null, true, false));
try { spl_autoload_register(fn($c) => null, false); echo "ok\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(spl_autoload_unregister(fn() => 1));
spl_autoload('Zzz', '.nope');
try { spl_autoload(''); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
?>
--EXPECTF--
A X1
B X2
int(2)
B Dyn
C Dyn
bool(true)
bool(true)
B Q
C Q
B 
C 
B Lead
C Lead
bool(false)
bool(false)
bool(false)
TypeError: spl_autoload_register(): Argument #1 ($callback) must be a valid callback or null, function "nonexistent_function_xyz" not found or invalid function name
TypeError: spl_autoload_register(): Argument #1 ($callback) must be a valid callback or null, class stdClass does not have a method "nope"
B L
C L
TypeError: spl_autoload_register(): Argument #1 ($callback) must be a valid callback or null, class "L" not found
bool(true)

Notice: spl_autoload_register(): Argument #2 ($do_throw) has been ignored, spl_autoload_register() will always throw in %s on line 18
ok
bool(false)
