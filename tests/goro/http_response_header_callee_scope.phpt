--TEST--
$http_response_header is set in the frame that called the wrapper, not in its callers
--INI--
allow_url_fopen=1
--FILE--
<?php
$code = <<<'SRV'
<?php
$srv = stream_socket_server('tcp://127.0.0.1:0');
echo stream_socket_get_name($srv, false), "\n";
for ($i = 0; $i < 4; $i++) {
    $c = stream_socket_accept($srv, 20);
    if (!$c) exit(1);
    fread($c, 8192);
    fwrite($c, "HTTP/1.1 200 OK\r\nX-N: $i\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    fclose($c);
}
SRV;
$f = tempnam(sys_get_temp_dir(), 'srv');
file_put_contents($f, $code);
$p = proc_open([PHP_BINARY, '-n', $f], [1 => ['pipe', 'w']], $pipes);
$base = "http://" . trim(fgets($pipes[1]));

function g($u) { return file_get_contents($u); }
function f($u) {
    $http_response_header = 'mine';
    g($u);
    return $http_response_header;
}
function h($u) {
    for ($i = 0; $i < 2; $i++) {
        g($u);
    }
    return isset($http_response_header);
}
var_dump(g("$base/a"));
var_dump(isset($http_response_header));
var_dump(f("$base/b"));
var_dump(h("$base/c"));
proc_close($p);
unlink($f);
?>
--EXPECT--
string(2) "ok"
bool(false)
string(4) "mine"
bool(false)
