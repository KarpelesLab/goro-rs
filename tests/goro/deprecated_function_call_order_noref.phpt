--TEST--
mhash*(), shmop_close(): the deprecation from the call, then the argument checks
--FILE--
<?php
foreach ([[], ['x', 'y'], [1, []]] as $a) {
    try { var_dump(mhash(...$a)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
try { var_dump(mhash_count(1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(mhash_get_block_size()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(mhash_keygen_s2k(1, 'a', 'b')); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(shmop_close()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(shmop_close(1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
set_error_handler(function ($n, $s) { throw new Exception($s); });
function w() { return mhash_count(); }
try { var_dump(w()); } catch (Exception $e) { echo $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
--EXPECTF--
Deprecated: Function mhash() is deprecated since 8.1 in %s on line 3
ArgumentCountError: mhash() expects at least 2 arguments, 0 given

Deprecated: Function mhash() is deprecated since 8.1 in %s on line 3
TypeError: mhash(): Argument #1 ($algo) must be of type int, string given

Deprecated: Function mhash() is deprecated since 8.1 in %s on line 3
TypeError: mhash(): Argument #2 ($data) must be of type string, array given

Deprecated: Function mhash_count() is deprecated since 8.1 in %s on line 5
ArgumentCountError: mhash_count() expects exactly 0 arguments, 1 given

Deprecated: Function mhash_get_block_size() is deprecated since 8.1 in %s on line 6
ArgumentCountError: mhash_get_block_size() expects exactly 1 argument, 0 given

Deprecated: Function mhash_keygen_s2k() is deprecated since 8.1 in %s on line 7
ArgumentCountError: mhash_keygen_s2k() expects exactly 4 arguments, 3 given

Deprecated: Function shmop_close() is deprecated since 8.0, as Shmop objects are freed automatically in %s on line 8
ArgumentCountError: shmop_close() expects exactly 1 argument, 0 given

Deprecated: Function shmop_close() is deprecated since 8.0, as Shmop objects are freed automatically in %s on line 9
TypeError: shmop_close(): Argument #1 ($shmop) must be of type Shmop, int given
Function mhash_count() is deprecated since 8.1
#0 %s(11): {closure:%s:10}(8192, 'Function mhash_...', '%s', 11)
#1 %s(12): w()
#2 {main}
