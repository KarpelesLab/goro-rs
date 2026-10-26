--TEST--
include falls back to the working directory; a file failing to compile is not in get_included_files() (unless _once)
--FILE--
<?php
$d = sys_get_temp_dir() . '/goro_inc_' . getmypid();
@mkdir("$d/sub", 0777, true);
file_put_contents("$d/peer.php", '<?php return "peer-in-d";');
file_put_contents("$d/sub/peer.php", '<?php return "peer-in-sub";');
file_put_contents("$d/bad.php", '<?php echo "a" "b";');
chdir($d);
set_include_path('/nonexistent');
var_dump(include 'peer.php');
var_dump(include 'sub/peer.php');
try { include 'bad.php'; } catch (ParseError $e) { echo get_class($e), "\n"; }
try { include_once 'bad.php'; } catch (ParseError $e) { echo get_class($e), "\n"; }
$inc = array_map('basename', get_included_files());
var_dump($inc);
unlink("$d/peer.php"); unlink("$d/sub/peer.php"); unlink("$d/bad.php"); rmdir("$d/sub"); rmdir($d);
--EXPECTF--
string(9) "peer-in-d"
string(11) "peer-in-sub"
ParseError
ParseError
array(4) {
  [0]=>
  string(%d) "%s"
  [1]=>
  string(8) "peer.php"
  [2]=>
  string(8) "peer.php"
  [3]=>
  string(7) "bad.php"
}
