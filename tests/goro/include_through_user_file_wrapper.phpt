--TEST--
include/require of a plain path goes through a user wrapper registered for file://
--FILE--
<?php
class W {
    public $context;
    private $h;
    private $p;
    public function stream_open(string $path, string $mode): bool {
        echo "open $mode ", basename($path), "\n";
        stream_wrapper_restore('file');
        $this->p = $path;
        return false !== $this->h = fopen('compress.zlib://' . $path, $mode);
    }
    public function stream_read(int $n): string|false { return fread($this->h, $n); }
    public function stream_eof(): bool { return feof($this->h); }
    public function stream_set_option(int $o, int $a, int $b): bool { return false; }
    public function stream_stat(): array|false { return stat($this->p); }
    public function stream_close() { echo "close\n"; fclose($this->h); }
}
$f = sys_get_temp_dir() . '/goro_fwrap_test.php';
file_put_contents($f, gzencode('<?php return ["a" => 1, "file" => basename(__FILE__)];'));
stream_wrapper_unregister('file');
stream_wrapper_register('file', W::class);
$r = require $f;
var_dump($r);
var_dump(in_array('file', stream_get_wrappers()));
unlink($f);

?>
--EXPECT--
open rb goro_fwrap_test.php
close
array(2) {
  ["a"]=>
  int(1)
  ["file"]=>
  string(19) "goro_fwrap_test.php"
}
bool(true)
