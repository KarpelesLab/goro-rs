--TEST--
mkdir('', recursive) Invalid path; copy('') ValueError; a closure parameter's declaring function is anonymous
--FILE--
<?php
var_dump(@mkdir('', 0777, true));
var_dump(error_get_last()['message'] ?? null);
try { var_dump(@copy('', '/tmp/goro_never_created_x')); } catch (ValueError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$p = (new ReflectionFunction(function ($param) {}))->getParameters()[0];
var_dump($p->getDeclaringFunction()->isAnonymous());
var_dump((new ReflectionFunction('strlen'))->getParameters()[0]->getDeclaringFunction()->isAnonymous());

?>
--EXPECT--
bool(false)
string(21) "mkdir(): Invalid path"
ValueError: Path must not be empty
bool(true)
bool(false)
