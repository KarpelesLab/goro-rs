--TEST--
cli-server: a "Proxy:" request header never becomes $_SERVER['HTTP_PROXY'] (httpoxy, bug #72573)
--SKIPIF--
<?php if (!function_exists('proc_open')) die('skip proc_open() not available'); ?>
--FILE--
<?php
$doc = sys_get_temp_dir() . '/r6_cli_server_px_' . getmypid();
@mkdir($doc);
file_put_contents("$doc/index.php", '<?php var_dump($_SERVER["HTTP_PROXY"] ?? "unset", getenv("HTTP_PROXY"));');
$log = tempnam(sys_get_temp_dir(), 'r6log');
$cmd = [getenv('TEST_PHP_EXECUTABLE') ?: PHP_BINARY, '-n', '-t', $doc, '-S', '127.0.0.1:0'];
foreach ([null, 'http://from-env:1'] as $proxy) {
    $env = ['PATH' => getenv('PATH')];
    if ($proxy) $env['HTTP_PROXY'] = $proxy;
    file_put_contents($log, '');
    $h = proc_open($cmd, [0 => ['pipe', 'r'], 1 => ['file', $log, 'a'], 2 => ['file', $log, 'a']], $pipes, $doc, $env);
    $addr = null;
    for ($i = 0; $i < 100 && !$addr; $i++) {
        usleep(50000);
        if (preg_match('@Development Server \(http://(.*?:\d+)\) started@', file_get_contents($log), $m)) $addr = $m[1];
    }
    $fp = fsockopen("tcp://$addr");
    fwrite($fp, "GET /index.php HTTP/1.0\r\nHost: x\r\nProxy: http://evil:2\r\n\r\n");
    $resp = stream_get_contents($fp);
    fclose($fp);
    proc_terminate($h);
    proc_close($h);
    echo substr($resp, strpos($resp, "\r\n\r\n") + 4);
}
unlink("$doc/index.php");
rmdir($doc);
unlink($log);
?>
--EXPECT--
string(5) "unset"
bool(false)
string(17) "http://from-env:1"
string(17) "http://from-env:1"
