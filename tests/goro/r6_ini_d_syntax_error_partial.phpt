--TEST--
INI syntax error in -d entries keeps the entries before it
--FILE--
<?php
// A syntax error in the -d entries: those before it (and the value parsed
// up to the error) still apply, later ones do not; the error goes to stderr.
$php = getenv('TEST_PHP_EXECUTABLE') ?: PHP_BINARY;
$code = 'var_dump(ini_get("precision"), ini_get("session.trans_sid_tags"), ini_get("serialize_precision"));';
foreach ([['session.trans_sid_tags=a=href,form='], ['session.trans_sid_tags="a=href'], ['session.trans_sid_tags=a', 'x[=1']] as $bad) {
    $cmd = [$php, '-n', '-d', 'precision=5'];
    foreach ($bad as $b) { $cmd[] = '-d'; $cmd[] = $b; }
    array_push($cmd, '-d', 'serialize_precision=3', '-d', 'error_reporting=0', '-r', $code);
    $h = proc_open($cmd, [1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
    echo "stdout: ", stream_get_contents($pipes[1]), "stderr: ", stream_get_contents($pipes[2]);
    proc_close($h);
}
--EXPECT--
stdout: 
Deprecated: PHP Startup: Usage of session.trans_sid_tags INI setting is deprecated in Unknown on line 0
string(1) "5"
string(1) "a"
string(2) "-1"
stderr: PHP:  syntax error, unexpected '=' in Unknown on line 7
stdout: string(1) "5"
string(32) "a=href,area=href,frame=src,form="
string(2) "-1"
stderr: PHP:  syntax error, unexpected end of file, expecting TC_DOLLAR_CURLY or TC_QUOTED_STRING or '"' in Unknown on line 10
stdout: 
Deprecated: PHP Startup: Usage of session.trans_sid_tags INI setting is deprecated in Unknown on line 0
string(1) "5"
string(1) "a"
string(2) "-1"
stderr: PHP:  syntax error, unexpected end of file, expecting ']' in Unknown on line 8
