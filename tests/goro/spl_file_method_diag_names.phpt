--TEST--
SplFileObject::fputcsv()/fscanf() report diagnostics under the method's name
--INI--
zend.exception_ignore_args=0
zend.exception_string_param_max_len=15
--FILE--
<?php
$f = new SplFileObject(__FILE__);
try { $f->flock(99); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($f->fputcsv(['a']));
var_dump($f->fwrite('abc'));
$t = new SplTempFileObject();
$t->fwrite("1 2\n");
$t->rewind();
var_dump($t->fscanf(null));
var_dump($t->fgetcsv(';', '"', '\\'));
try { $t->fscanf([]); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECTF--

Deprecated: SplFileObject::fputcsv(): the $escape parameter must be provided, as its default value will change, either explicitly or via SplFileObject::setCsvControl() in %s on line 4

Notice: SplFileObject::fputcsv(): Write of 2 bytes failed with errno=9 Bad file descriptor in %s on line 4
bool(false)

Notice: SplFileObject::fwrite(): Write of 3 bytes failed with errno=9 Bad file descriptor in %s on line 5
bool(false)

Deprecated: SplFileObject::fscanf(): Passing null to parameter #1 ($format) of type string is deprecated in %s on line 9
array(0) {
}
bool(false)
SplFileObject::fscanf(): Argument #1 ($format) must be of type string, array given
