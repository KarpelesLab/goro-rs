--TEST--
$_SESSION set up by session_start() inside a function is the global the function sees
--FILE--
<?php
function load() {
    $session = &$_SESSION;
    $session['k'] = [1];
}
function t($dir) {
    $before = isset($_SESSION);
    ini_set('session.save_path', $dir);
    session_start();
    $_SESSION['drak'] = 'x';
    load();
    $GLOBALS['_SESSION']['g'] = 2;
    var_dump($before, $_SESSION);
    session_destroy();
}
$dir = sys_get_temp_dir() . '/goro_sess_sg_' . getmypid();
@mkdir($dir);
t($dir);
@rmdir($dir);

?>
--EXPECT--
bool(false)
array(3) {
  ["drak"]=>
  string(1) "x"
  ["k"]=>
  array(1) {
    [0]=>
    int(1)
  }
  ["g"]=>
  int(2)
}
