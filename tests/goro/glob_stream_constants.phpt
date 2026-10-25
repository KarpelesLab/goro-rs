--TEST--
GLOB_* have the bundled php_glob()'s values; glob() rejects unknown flags; missing stream constants
--FILE--
<?php
foreach (['GLOB_BRACE', 'GLOB_ERR', 'GLOB_MARK', 'GLOB_NOCHECK', 'GLOB_NOESCAPE', 'GLOB_NOSORT', 'GLOB_ONLYDIR', 'GLOB_AVAILABLE_FLAGS',
          'FILE_NO_DEFAULT_CONTEXT', 'STREAM_IGNORE_URL', 'STREAM_MUST_SEEK', 'STREAM_PF_INET6', 'STREAM_SOCK_RDM', 'STREAM_SOCK_SEQPACKET'] as $c) {
    echo $c, "=", constant($c), "\n";
}
$d = sys_get_temp_dir() . "/goro_globc_" . getmypid();
mkdir($d); mkdir("$d/sub"); touch("$d/b.txt"); touch("$d/a.txt");
var_dump(array_map('basename', glob("$d/*", GLOB_MARK)));
var_dump(glob("$d/sub", GLOB_MARK) === ["$d/sub/"]);
var_dump(glob("$d/*.none", GLOB_NOCHECK) === ["$d/*.none"]);
var_dump(count(glob("$d/{a,b}.txt", GLOB_BRACE)));
var_dump(glob("$d/*", 1 << 20));
var_dump(stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_SEQPACKET, 0) !== false);
unlink("$d/a.txt"); unlink("$d/b.txt"); rmdir("$d/sub"); rmdir($d);
?>
--EXPECTF--
GLOB_BRACE=128
GLOB_ERR=4
GLOB_MARK=8
GLOB_NOCHECK=16
GLOB_NOESCAPE=4096
GLOB_NOSORT=32
GLOB_ONLYDIR=1073741824
GLOB_AVAILABLE_FLAGS=1073746108
FILE_NO_DEFAULT_CONTEXT=16
STREAM_IGNORE_URL=2
STREAM_MUST_SEEK=16
STREAM_PF_INET6=10
STREAM_SOCK_RDM=4
STREAM_SOCK_SEQPACKET=5
array(3) {
  [0]=>
  string(5) "a.txt"
  [1]=>
  string(5) "b.txt"
  [2]=>
  string(3) "sub"
}
bool(true)
bool(true)
int(2)

Warning: glob(): At least one of the passed flags is invalid or not supported on this platform in %s on line %d
bool(false)
bool(true)
