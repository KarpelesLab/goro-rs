--TEST--
Directory handles are streams: stream functions on them
--FILE--
<?php
mkdir(__DIR__ . "/dirstream_ops_dd"); touch(__DIR__ . "/dirstream_ops_dd/x");
$d = opendir(__DIR__ . "/dirstream_ops_dd");
foreach (['fread' => fn() => fread($d, 1000), 'fgets' => fn() => fgets($d), 'fgetc' => fn() => fgetc($d),
  'feof' => fn() => feof($d), 'ftell' => fn() => ftell($d), 'fseek' => fn() => fseek($d, 0), 'rewind' => fn() => rewind($d),
  'fflush' => fn() => fflush($d), 'fstat' => fn() => is_array(fstat($d)), 'ftruncate' => fn() => ftruncate($d, 0),
  'flock' => fn() => flock($d, LOCK_SH), 'sgc' => fn() => stream_get_contents($d), 'fpassthru' => fn() => fpassthru($d),
  'meta' => fn() => stream_get_meta_data($d), 'fputs' => fn() => fputs($d, "x"), 'sgl' => fn() => stream_get_line($d, 10),
  'set_blocking' => fn() => stream_set_blocking($d, true), 'readdir' => fn() => readdir($d), 'fscanf' => fn() => fscanf($d, "%s"),
  'fgetcsv' => fn() => fgetcsv($d), 'fputcsv' => fn() => fputcsv($d, ['a']), 'is_local' => fn() => stream_is_local($d),
  'isatty' => fn() => stream_isatty($d), 'copy' => fn() => stream_copy_to_stream($d, fopen("php://memory", "w")),
  ] as $k => $f) {
  echo "$k: "; try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
echo "close: "; var_dump(fclose($d));
var_dump(readdir($d));
closedir($d);
unlink(__DIR__ . "/dirstream_ops_dd/x"); rmdir(__DIR__ . "/dirstream_ops_dd");
--EXPECTF--
fread: bool(false)
fgets: bool(false)
fgetc: bool(false)
feof: bool(false)
ftell: int(0)
fseek: int(0)
rewind: bool(true)
fflush: bool(true)
fstat: bool(false)
ftruncate: 
Warning: ftruncate(): Can't truncate this stream! in %s on line 6
bool(false)
flock: bool(false)
sgc: string(0) ""
fpassthru: int(-1)
meta: array(8) {
  ["timed_out"]=>
  bool(false)
  ["blocked"]=>
  bool(true)
  ["eof"]=>
  bool(false)
  ["wrapper_type"]=>
  string(9) "plainfile"
  ["stream_type"]=>
  string(3) "dir"
  ["mode"]=>
  string(1) "r"
  ["unread_bytes"]=>
  int(0)
  ["seekable"]=>
  bool(true)
}
fputs: 
Notice: fputs(): Stream is not writable in %s on line 8
bool(false)
sgl: bool(false)
set_blocking: bool(true)
readdir: string(1) "."
fscanf: bool(false)
fgetcsv: 
Deprecated: fgetcsv(): the $escape parameter must be provided as its default value will change in %s on line 10
bool(false)
fputcsv: 
Deprecated: fputcsv(): the $escape parameter must be provided as its default value will change in %s on line 10

Notice: fputcsv(): Stream is not writable in %s on line 10
bool(false)
is_local: bool(true)
isatty: bool(false)
copy: bool(false)
close: 
Warning: fclose(): cannot close the provided stream, as it must not be manually closed in %s on line 15
bool(false)
string(2) ".."
