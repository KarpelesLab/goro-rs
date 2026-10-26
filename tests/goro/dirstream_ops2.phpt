--TEST--
More stream functions on directory handles; pclose() of any stream; gz* / readdir() argument counts
--FILE--
<?php
$d = opendir(__DIR__);
$m = fopen("php://memory", "w+");
foreach (['stream_set_timeout' => fn() => stream_set_timeout($d, 1), 'stream_set_chunk_size' => fn() => stream_set_chunk_size($d, 100),
  'stream_set_read_buffer' => fn() => stream_set_read_buffer($d, 0), 'stream_set_write_buffer' => fn() => stream_set_write_buffer($d, 0),
  'stream_supports_lock' => fn() => stream_supports_lock($d), 'stream_socket_get_name' => fn() => stream_socket_get_name($d, false),
  'stream_socket_shutdown' => fn() => stream_socket_shutdown($d, 1), 'fprintf' => fn() => fprintf($d, "x"), 'vfprintf' => fn() => vfprintf($d, "x", []),
  'gztell' => fn() => gztell($d), 'gzeof' => fn() => gzeof($d), 'gzread' => fn() => gzread($d, 10), 'gzgets' => fn() => gzgets($d),
  'gzwrite' => fn() => gzwrite($d, "x"), 'gzrewind' => fn() => gzrewind($d), 'gzseek' => fn() => gzseek($d, 0), 'gzpassthru' => fn() => gzpassthru($d),
  'pclose_m' => fn() => pclose($m),
  ] as $k => $f) {
  echo "$k: "; try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
foreach (['gztell', 'gzeof', 'gzrewind', 'gzpassthru', 'gzclose', 'readdir', 'rewinddir', 'closedir'] as $f) {
  try { $f($d, 1); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
try { gzread($m, 1, 1); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { gzseek($m, 1, 1, 1); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { gzgets($m, 1, 1); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { gzwrite($m, 1, 1, 1); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$m = fopen("php://memory", "w+"); var_dump(pclose($m)); var_dump(is_resource($m));
$f = fopen(__FILE__, "r"); var_dump(pclose($f), get_resource_type($f));
$t = tmpfile(); var_dump(pclose($t));
$d = opendir("."); var_dump(pclose($d), get_resource_type($d));
--EXPECTF--
stream_set_timeout: bool(false)
stream_set_chunk_size: int(8192)
stream_set_read_buffer: int(0)
stream_set_write_buffer: int(-1)
stream_supports_lock: bool(false)
stream_socket_get_name: bool(false)
stream_socket_shutdown: bool(false)
fprintf: 
Notice: fprintf(): Stream is not writable in %s on line 7
int(1)
vfprintf: 
Notice: vfprintf(): Stream is not writable in %s on line 7
int(1)
gztell: int(0)
gzeof: bool(false)
gzread: bool(false)
gzgets: bool(false)
gzwrite: 
Notice: gzwrite(): Stream is not writable in %s on line 9
bool(false)
gzrewind: bool(true)
gzseek: int(0)
gzpassthru: int(-1)
pclose_m: int(0)
ArgumentCountError: gztell() expects exactly 1 argument, 2 given
ArgumentCountError: gzeof() expects exactly 1 argument, 2 given
ArgumentCountError: gzrewind() expects exactly 1 argument, 2 given
ArgumentCountError: gzpassthru() expects exactly 1 argument, 2 given
ArgumentCountError: gzclose() expects exactly 1 argument, 2 given
ArgumentCountError: readdir() expects at most 1 argument, 2 given
ArgumentCountError: rewinddir() expects at most 1 argument, 2 given
ArgumentCountError: closedir() expects at most 1 argument, 2 given
ArgumentCountError: gzread() expects exactly 2 arguments, 3 given
ArgumentCountError: gzseek() expects at most 3 arguments, 4 given
ArgumentCountError: gzgets() expects at most 2 arguments, 3 given
ArgumentCountError: gzwrite() expects at most 3 arguments, 4 given
int(0)
bool(false)
int(0)
string(7) "Unknown"
int(0)
int(0)
string(7) "Unknown"
