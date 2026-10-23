--TEST--
Stream functions reject non-resource arguments with the zpp type error
--FILE--
<?php
// Stream functions reject non-resource arguments (PHP_Z_PARAM_STREAM)
foreach ([fn() => fclose("10"), fn() => fread("x", 1), fn() => fwrite(1, "x"), fn() => feof(null), fn() => fgets([]),
          fn() => ftell(1.5), fn() => rewind(true), fn() => gzclose('10'), fn() => gzread(null, 1), fn() => fseek(1, 0),
          fn() => fflush('x'), fn() => fstat(2), fn() => ftruncate(3, 0), fn() => flock(1, LOCK_SH), fn() => fpassthru(2),
          fn() => fgetc(1), fn() => fgetcsv(1), fn() => fputcsv(1, []), fn() => stream_get_contents(1), fn() => stream_get_meta_data(1),
          fn() => fputs(2, 'x'), fn() => fscanf(1, '%d')] as $f) {
    try { var_dump($f()); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
}
$h = fopen('php://memory', 'w+');
var_dump(fwrite($h, 'ok'), fclose($h));
var_dump(fwrite(STDOUT, "to stdout\n"));
--EXPECT--
fclose(): Argument #1 ($stream) must be of type resource, string given
fread(): Argument #1 ($stream) must be of type resource, string given
fwrite(): Argument #1 ($stream) must be of type resource, int given
feof(): Argument #1 ($stream) must be of type resource, null given
fgets(): Argument #1 ($stream) must be of type resource, array given
ftell(): Argument #1 ($stream) must be of type resource, float given
rewind(): Argument #1 ($stream) must be of type resource, true given
gzclose(): Argument #1 ($stream) must be of type resource, string given
gzread(): Argument #1 ($stream) must be of type resource, null given
fseek(): Argument #1 ($stream) must be of type resource, int given
fflush(): Argument #1 ($stream) must be of type resource, string given
fstat(): Argument #1 ($stream) must be of type resource, int given
ftruncate(): Argument #1 ($stream) must be of type resource, int given
flock(): Argument #1 ($stream) must be of type resource, int given
fpassthru(): Argument #1 ($stream) must be of type resource, int given
fgetc(): Argument #1 ($stream) must be of type resource, int given
fgetcsv(): Argument #1 ($stream) must be of type resource, int given
fputcsv(): Argument #1 ($stream) must be of type resource, int given
stream_get_contents(): Argument #1 ($stream) must be of type resource, int given
stream_get_meta_data(): Argument #1 ($stream) must be of type resource, int given
fputs(): Argument #1 ($stream) must be of type resource, int given
fscanf(): Argument #1 ($stream) must be of type resource, int given
int(2)
bool(true)
to stdout
int(10)
