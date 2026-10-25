--TEST--
cli-server: the query string and request headers go through the input filter (INPUT_SERVER, filter.default)
--SKIPIF--
<?php if (!function_exists('proc_open')) die('skip proc_open() not available'); ?>
--FILE--
<?php
$doc = sys_get_temp_dir() . '/r6_cli_server_if_' . getmypid();
@mkdir($doc);
file_put_contents("$doc/index.php", '<?php $a = filter_input_array(INPUT_SERVER); ksort($a); var_dump($a, $_SERVER["HTTP_USER_AGENT"], $_SERVER["QUERY_STRING"], $_SERVER["REQUEST_URI"]);');
$log = tempnam(sys_get_temp_dir(), 'r6log');
$cmd = [getenv('TEST_PHP_EXECUTABLE') ?: PHP_BINARY, '-n', '-d', 'filter.default=special_chars',
        '-t', $doc, '-S', '127.0.0.1:0'];
$h = proc_open($cmd, [0 => ['pipe', 'r'], 1 => ['file', $log, 'a'], 2 => ['file', $log, 'a']], $pipes, $doc);
$addr = null;
for ($i = 0; $i < 100 && !$addr; $i++) {
    usleep(50000);
    if (preg_match('@Development Server \(http://(.*?:\d+)\) started@', file_get_contents($log), $m)) $addr = $m[1];
}
$fp = fsockopen("tcp://$addr");
fwrite($fp, "GET /index.php?a=<x> HTTP/1.0\r\nHost: x\r\nUser-Agent: <b>\r\n\r\n");
$resp = stream_get_contents($fp);
fclose($fp);
proc_terminate($h);
proc_close($h);
echo substr($resp, strpos($resp, "\r\n\r\n") + 4);
unlink("$doc/index.php");
rmdir($doc);
unlink($log);
?>
--EXPECT--
array(3) {
  ["HTTP_HOST"]=>
  string(1) "x"
  ["HTTP_USER_AGENT"]=>
  string(3) "<b>"
  ["QUERY_STRING"]=>
  string(5) "a=<x>"
}
string(11) "&#60;b&#62;"
string(13) "a=&#60;x&#62;"
string(16) "/index.php?a=<x>"
