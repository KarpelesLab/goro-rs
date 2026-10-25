--TEST--
User wrappers: md5_file()/sha1_file()/getimagesize() open without a context; fgetcsv() checks $escape first
--FILE--
<?php
class W {
    public $context;
    private $pos = 0;
    function stream_open($path, $mode, $options, &$opened_path) {
        echo "open ", is_resource($this->context) ? get_resource_type($this->context) : var_export($this->context, true), "\n";
        return true;
    }
    function stream_read($n) { $d = substr("a,b\nc,d\n", $this->pos, $n); $this->pos += strlen($d); return $d; }
    function stream_eof() { return $this->pos >= 8; }
    function stream_stat() { return []; }
}
stream_wrapper_register('wx', 'W');
var_dump(md5_file('wx://a'));
var_dump(sha1_file('wx://a'));
var_dump(@getimagesize('wx://a'));
var_dump(hash_file('crc32b', 'wx://a'));
var_dump(strlen(file_get_contents('wx://a')));
$f = fopen('wx://a', 'r');
var_dump(fgetcsv($f));
try { fgetcsv($f, -1); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
var_dump(fgetcsv($f, 0, ",", '"', ""));
?>
--EXPECTF--
open NULL
string(32) "46cd49ecda153678ad058e9b171ddf85"
open NULL
string(40) "da89ad46b857f7d21f5ef853e22d9f8f6cc004be"
open NULL
bool(false)
open NULL
string(8) "87723a80"
open stream-context
int(8)
open stream-context

Deprecated: fgetcsv(): the $escape parameter must be provided as its default value will change in %s on line %d
array(2) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
}

Deprecated: fgetcsv(): the $escape parameter must be provided as its default value will change in %s on line %d
fgetcsv(): Argument #2 ($length) must be between 0 and 9223372036854775806
array(2) {
  [0]=>
  string(1) "c"
  [1]=>
  string(1) "d"
}
