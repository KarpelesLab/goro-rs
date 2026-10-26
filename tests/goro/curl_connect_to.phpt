--TEST--
CURLOPT_CONNECT_TO redirects the connection, keeping the URL's Host
--EXTENSIONS--
curl
pcntl
--FILE--
<?php
$srv = stream_socket_server('tcp://127.0.0.1:0');
$port = (int)substr(strrchr(stream_socket_get_name($srv, false), ':'), 1);
$pid = pcntl_fork();
if ($pid === 0) {
    for ($i = 0; $i < 4; $i++) {
        $c = stream_socket_accept($srv, 10);
        $req = '';
        while (!str_contains($req, "\r\n\r\n")) $req .= fread($c, 4096);
        preg_match('/^Host: (.*)\r$/mi', $req, $m);
        $body = "host=" . $m[1];
        fwrite($c, "HTTP/1.1 200 OK\r\nContent-Length: " . strlen($body) . "\r\nConnection: close\r\n\r\n$body");
        fclose($c);
    }
    exit(0);
}
fclose($srv);
$cases = [
    ["http://other.test/x", ["other.test:80:127.0.0.1:$port"]],
    ["http://other.test/x", ["nomatch.test::127.0.0.1:1", "other.test:81:127.0.0.1:1", ":80:127.0.0.1:$port"]],
    ["http://127.0.0.1:1/x", ["127.0.0.1:1::$port"]],
    ["http://OTHER.test:8080/x", ["other.test::127.0.0.1:$port"]],
    ["http://127.0.0.1:1/x", ["::127.0.0.1:abc"]],
];
foreach ($cases as [$url, $ct]) {
    $err = fopen('php://memory', 'w+');
    $ch = curl_init($url);
    curl_setopt_array($ch, [CURLOPT_RETURNTRANSFER => 1, CURLOPT_CONNECT_TO => $ct, CURLOPT_VERBOSE => 1, CURLOPT_STDERR => $err, CURLOPT_TIMEOUT => 5]);
    $r = curl_exec($ch);
    echo json_encode($r), ' ', curl_errno($ch), ' [', curl_error($ch), "]\n";
    rewind($err);
    foreach (explode("\n", stream_get_contents($err)) as $l) {
        if (preg_match('/^\* (Connecting to|No valid)/', $l)) echo "  ", str_replace((string)$port, 'PORT', $l), "\n";
    }
}
pcntl_waitpid($pid, $st);
?>
--EXPECT--
"host=other.test" 0 []
  * Connecting to hostname: 127.0.0.1
  * Connecting to port: PORT
"host=other.test" 0 []
  * Connecting to hostname: 127.0.0.1
  * Connecting to port: PORT
"host=127.0.0.1:1" 0 []
  * Connecting to port: PORT
"host=OTHER.test:8080" 0 []
  * Connecting to hostname: 127.0.0.1
  * Connecting to port: PORT
false 49 [No valid port number in connect to host string (abc)]
  * No valid port number in connect to host string (abc)
