--TEST--
stream_get_contents() offset on a read-filtered stream: within the read buffer, else a seek dropping it
--FILE--
<?php
class F extends php_user_filter {
    private string $buffer = '';
    public function filter($in, $out, &$consumed, bool $closing): int {
        $b = $this->buffer;
        while (null !== ($bucket = stream_bucket_make_writeable($in))) {
            $b .= $bucket->data;
            $consumed += $bucket->datalen;
        }
        if ('' === $b && !$closing) return PSFS_FEED_ME;
        if (!mb_check_encoding($b, 'UTF-8') && !$closing) { $this->buffer = $b; return PSFS_FEED_ME; }
        stream_bucket_append($out, stream_bucket_new($this->stream, $b));
        $this->buffer = '';
        return PSFS_PASS_ON;
    }
}
stream_filter_register('f', 'F');
$file = tmpfile();
stream_filter_append($file, 'f', STREAM_FILTER_READ);
fwrite($file, "\u{1F4A9}");
fflush($file);
rewind($file);
var_dump(bin2hex(stream_get_contents($file, length: 2, offset: 0)));
var_dump(bin2hex(stream_get_contents($file, offset: 2)));
rewind($file);
var_dump(bin2hex(stream_get_contents($file)));
$f2 = tmpfile();
fwrite($f2, "abcdefgh");
stream_filter_append($f2, 'string.toupper', STREAM_FILTER_READ);
rewind($f2);
var_dump(fread($f2, 1));
var_dump(stream_get_contents($f2, 2, 5));
var_dump(stream_get_contents($f2, -1, 1));
var_dump(stream_get_contents($f2, 3, 0));

?>
--EXPECT--
string(4) "f09f"
string(4) "92a9"
string(8) "f09f92a9"
string(1) "A"
string(2) "FG"
string(7) "BCDEFGH"
string(3) "ABC"
