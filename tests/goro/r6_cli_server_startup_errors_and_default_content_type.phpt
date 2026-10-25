--TEST--
cli-server: module startup errors stay out of responses; headers_list() gets the default Content-type once sent
--SKIPIF--
<?php if (!function_exists('proc_open')) die('skip proc_open() not available'); ?>
--FILE--
<?php
$doc = sys_get_temp_dir() . '/r6_cli_server_' . getmypid();
@mkdir($doc);
file_put_contents("$doc/index.php", '<?php header("X-A: b"); var_dump(headers_list()); echo "out\n"; var_dump(headers_list());');
$log = tempnam(sys_get_temp_dir(), 'r6log');
$cmd = [getenv('TEST_PHP_EXECUTABLE') ?: PHP_BINARY, '-n', '-d', 'session.name=1', '-d', 'session.sid_length=40',
        '-d', 'html_errors=0', '-t', $doc, '-S', '127.0.0.1:0'];
$h = proc_open($cmd, [0 => ['pipe', 'r'], 1 => ['file', $log, 'a'], 2 => ['file', $log, 'a']], $pipes, $doc);
$addr = null;
for ($i = 0; $i < 100 && !$addr; $i++) {
    usleep(50000);
    if (preg_match('@Development Server \(http://(.*?:\d+)\) started@', file_get_contents($log), $m)) $addr = $m[1];
}
$fp = fsockopen("tcp://$addr");
fwrite($fp, "GET /index.php HTTP/1.0\r\nHost: x\r\n\r\n");
$resp = stream_get_contents($fp);
fclose($fp);
proc_terminate($h);
proc_close($h);
echo substr($resp, strpos($resp, "\r\n\r\n") + 4);
unlink("$doc/index.php");
rmdir($doc);
unlink($log);
?>
--EXPECTF--
array(2) {
  [0]=>
  string(%d) "X-Powered-By: PHP/%s"
  [1]=>
  string(6) "X-A: b"
}
out
array(3) {
  [0]=>
  string(%d) "X-Powered-By: PHP/%s"
  [1]=>
  string(6) "X-A: b"
  [2]=>
  string(38) "Content-type: text/html; charset=UTF-8"
}
