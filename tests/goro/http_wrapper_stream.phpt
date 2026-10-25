--TEST--
http:// wrapper: the stream is the socket of the request, positioned at the body
--INI--
allow_url_fopen=1
--FILE--
<?php
// A tiny HTTP server in another process: one canned response per request
// ("{REQ}" is replaced by the request received).
function serve(array $responses) {
    $code = <<<'SRV'
    <?php
    $responses = unserialize(file_get_contents($argv[1]));
    $srv = stream_socket_server('tcp://127.0.0.1:0');
    echo stream_socket_get_name($srv, false), "\n";
    foreach ($responses as $r) {
        $c = stream_socket_accept($srv, 20);
        if (!$c) exit(1);
        $req = '';
        while (!str_contains($req, "\r\n\r\n") && ($d = fread($c, 8192)) !== false && $d !== '') $req .= $d;
        if (preg_match('/^Content-Length: (\d+)/mi', $req, $m)) {
            [$h, $b] = explode("\r\n\r\n", $req, 2);
            while (strlen($b) < $m[1] && ($d = fread($c, 8192)) !== false && $d !== '') $b .= $d;
            $req = "$h\r\n\r\n$b";
        }
        fwrite($c, str_replace('{REQ}', $req, $r));
        fclose($c);
    }
    SRV;
    $f = tempnam(sys_get_temp_dir(), 'srv');
    file_put_contents($f, $code);
    file_put_contents("$f.data", serialize($responses));
    $p = proc_open([PHP_BINARY, '-n', $f, "$f.data"], [1 => ['pipe', 'w']], $pipes);
    $addr = trim(fgets($pipes[1]));
    return [$p, $f, "http://$addr"];
}
function ok($body, $extra = '') {
    return "HTTP/1.1 200 OK\r\nContent-Length: " . strlen($body) . "\r\n{$extra}Connection: close\r\n\r\n$body";
}
$body = str_repeat("0123456789", 3);
[$p, $f, $base] = serve([
    ok($body, "X-Foo: bar\r\n"),
    ok($body),
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n7\r\n world!\r\n0\r\n\r\n",
    ok("line1\nline2\n"),
    ok("copied"),
    ok($body),
    "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n{REQ}",
    ok("touched"),
]);

$fp = fopen("$base/a", "r");
$md = stream_get_meta_data($fp);
var_dump($md['wrapper_type'], $md['mode'], $md['unread_bytes'], $md['seekable'], $md['wrapper_data'][2]);
var_dump(fstat($fp));
var_dump(fread($fp, 5), ftell($fp));
var_dump(fseek($fp, 2, SEEK_CUR), ftell($fp), fread($fp, 3));
var_dump(fseek($fp, 12), fread($fp, 2));
var_dump(fseek($fp, 0));
var_dump(fseek($fp, 5, SEEK_END));
var_dump(stream_get_contents($fp), feof($fp));
fclose($fp);

// file_get_contents() seeks with SEEK_SET / SEEK_END
var_dump(file_get_contents("$base/b", false, null, 3, 4));

// chunked: the dechunk filter, past what was buffered with the headers
$fp = fopen("$base/c", "rb");
var_dump(fread($fp, 3), stream_get_contents($fp));
fclose($fp);

var_dump(file("$base/d"));
$m = fopen("php://memory", "w+");
var_dump(copy("$base/e", "php://memory"));
var_dump(file_get_contents("$base/f", false, null, -5));

// writes go to the socket whatever the mode
$fp = fopen("$base/g", "r");
var_dump(fwrite($fp, "x"));
fclose($fp);

// writeable modes and the operations the wrapper does not implement
var_dump(fopen("$base/h", "w"));
var_dump(fopen("$base/h", "r+"));
var_dump(file_put_contents("$base/h", "data"));
var_dump(unlink("$base/h"));
var_dump(rename("$base/h", "$base/i"));
var_dump(mkdir("$base/h"), rmdir("$base/h"));
var_dump(opendir("$base/h"));
var_dump(file_exists("$base/h"), is_file("$base/h"));
var_dump(chmod("$base/h", 0644));
var_dump(touch("$base/h", 1));
var_dump(touch("$base/h"));

proc_close($p);
unlink($f);
unlink("$f.data");
?>
--EXPECTF--
string(4) "http"
string(1) "r"
int(30)
bool(false)
string(10) "X-Foo: bar"
bool(false)
string(5) "01234"
int(5)
int(0)
int(7)
string(3) "789"
int(0)
string(2) "23"

Warning: fseek(): Stream does not support seeking in %s on line %d
int(-1)

Warning: fseek(): Stream does not support seeking in %s on line %d
int(-1)
string(16) "4567890123456789"
bool(true)
string(4) "3456"
string(3) "hel"
string(9) "lo world!"
array(2) {
  [0]=>
  string(6) "line1
"
  [1]=>
  string(6) "line2
"
}
bool(true)

Warning: file_get_contents(): Stream does not support seeking in %s on line %d

Warning: file_get_contents(): Failed to seek to position -5 in the stream in %s on line %d
bool(false)
int(1)

Warning: fopen(http://127.0.0.1:%d/h): Failed to open stream: HTTP wrapper does not support writeable connections in %s on line %d
bool(false)

Warning: fopen(http://127.0.0.1:%d/h): Failed to open stream: HTTP wrapper does not support writeable connections in %s on line %d
bool(false)

Warning: file_put_contents(http://127.0.0.1:%d/h): Failed to open stream: HTTP wrapper does not support writeable connections in %s on line %d
bool(false)

Warning: unlink(): http does not allow unlinking in %s on line %d
bool(false)

Warning: rename(): http wrapper does not support renaming in %s on line %d
bool(false)
bool(false)
bool(false)

Warning: opendir(http://127.0.0.1:%d/h): Failed to open directory: not implemented in %s on line %d
bool(false)
bool(false)
bool(false)

Warning: chmod(): Cannot call chmod() for a non-standard stream in %s on line %d
bool(false)

Warning: touch(): Cannot call touch() for a non-standard stream in %s on line %d
bool(false)
bool(true)
