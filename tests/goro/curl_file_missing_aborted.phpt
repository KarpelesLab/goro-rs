--TEST--
curl: a CURLFile that cannot be opened aborts the upload (CURLE_ABORTED_BY_CALLBACK)
--EXTENSIONS--
curl
--FILE--
<?php
$code = <<<'SRV'
<?php
$srv = stream_socket_server('tcp://127.0.0.1:0');
echo stream_socket_get_name($srv, false), "\n";
$c = stream_socket_accept($srv, 20);
if ($c) {
    // (read until the client gives up on the request)
    stream_set_timeout($c, 5);
    while (($d = fread($c, 8192)) !== false && $d !== '') {}
    fwrite($c, "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    fclose($c);
}
SRV;
$f = tempnam(sys_get_temp_dir(), 'srv');
file_put_contents($f, $code);
$p = proc_open([PHP_BINARY, '-n', $f], [1 => ['pipe', 'w']], $pipes);
$addr = trim(fgets($pipes[1]));

$ch = curl_init("http://$addr/upload");
curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
curl_setopt($ch, CURLOPT_POSTFIELDS, ['up' => new CURLFile('/nonexistent/goro/file', 'text/plain')]);
var_dump(curl_exec($ch), curl_errno($ch), curl_error($ch));
proc_close($p);
unlink($f);
?>
--EXPECT--
bool(false)
int(42)
string(29) "operation aborted by callback"
