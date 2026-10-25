--TEST--
Included file: early-bound linking errors come before a later compile error, an earlier compile error wins
--FILE--
<?php
$f = __DIR__ . '/early_binding_error_before_compile_error_include.inc';
file_put_contents($f, '<?php
class A { protected int $b = 5; }
class B extends A { private static int $b = 5; }
class C extends B { public static ?string $b = 1; }
');
$g = __DIR__ . '/early_binding_error_before_compile_error_include2.inc';
file_put_contents($g, '<?php
class E2 extends Exception {}
function f(int $x = "a") {}
class F2 extends Exception { final public function getMessage(): string { return ""; } }
');
$php = getenv('TEST_PHP_EXECUTABLE');
echo shell_exec(escapeshellarg($php) . ' -n -d display_errors=1 -r ' . escapeshellarg('include "' . $g . '";'));
echo "--\n";
include $f;
echo "unreached\n";
?>
--CLEAN--
<?php
@unlink(__DIR__ . '/early_binding_error_before_compile_error_include.inc');
@unlink(__DIR__ . '/early_binding_error_before_compile_error_include2.inc');
?>
--EXPECTF--

Fatal error: Cannot use string as default value for parameter $x of type int in %sinclude2.inc on line 3
Stack trace:
#0 {main}
--

Fatal error: Cannot redeclare non static A::$b as static B::$b in %sinclude.inc on line 3
