--TEST--
curl_getinfo() from a header/write/progress callback sees the running transfer (symfony/http-client's CurlResponse)
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
    stream_set_timeout($c, 5);
    $req = '';
    while (!str_contains($req, "\r\n\r\n") && ($d = fread($c, 8192)) !== false && $d !== '') { $req .= $d; }
    fwrite($c, "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
    fclose($c);
}
SRV;
$f = tempnam(sys_get_temp_dir(), 'srv');
file_put_contents($f, $code);
$p = proc_open([PHP_BINARY, '-n', $f], [1 => ['pipe', 'w']], $pipes);
$addr = trim(fgets($pipes[1]));

$url = "http://$addr/path";
$ch = curl_init($url);
curl_setopt($ch, CURLOPT_PRIVATE, 'H');
curl_setopt($ch, CURLOPT_HEADERFUNCTION, function ($ch, $h) use ($url) {
    if (str_starts_with($h, 'HTTP/') || $h === "\r\n") {
        echo json_encode(trim($h)), ' code=', curl_getinfo($ch, CURLINFO_RESPONSE_CODE),
            ' url=', var_export(curl_getinfo($ch, CURLINFO_EFFECTIVE_URL) === $url, true),
            ' private=', curl_getinfo($ch, CURLINFO_PRIVATE), "\n";
    }
    if ($h === "\r\n") curl_setopt($ch, CURLOPT_PRIVATE, 'C');
    return strlen($h);
});
curl_setopt($ch, CURLOPT_WRITEFUNCTION, function ($ch, $d) {
    echo 'write ', $d, ' code=', curl_getinfo($ch, CURLINFO_RESPONSE_CODE), ' type=', curl_getinfo($ch)['content_type'], ' private=', curl_getinfo($ch, CURLINFO_PRIVATE), "\n";
    return strlen($d);
});
$seen = false;
curl_setopt($ch, CURLOPT_NOPROGRESS, false);
curl_setopt($ch, CURLOPT_XFERINFOFUNCTION, function ($ch, $dt, $dn) use (&$seen, $url) {
    if ($dn > 0 && !$seen) { $seen = true; echo 'progress url ok=', var_export(curl_getinfo($ch)['url'] === $url, true), "\n"; }
    return 0;
});
curl_exec($ch);
echo 'final code=', curl_getinfo($ch, CURLINFO_RESPONSE_CODE), "\n";
proc_close($p);
unlink($f);
?>
--EXPECT--
"HTTP\/1.1 201 Created" code=201 url=true private=H
"" code=201 url=true private=H
write hello code=201 type=text/plain private=C
progress url ok=true
final code=201
