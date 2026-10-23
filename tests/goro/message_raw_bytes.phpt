--TEST--
Warning and exception messages keep bytes that are not UTF-8
--FILE--
<?php
set_error_handler(function ($no, $str) { echo "E: ", bin2hex($str), "\n"; return true; });
preg_match("/a/\xff", "a");
preg_match("\xff", "a");
date_default_timezone_set("\xfe");
function show(callable $f) {
    try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", bin2hex($e->getMessage()), "\n"; }
}
show(fn() => mb_convert_kana("abc", "\xff"));
show(fn() => mb_strlen("a", "\xff"));
show(fn() => new ReflectionClass("\xff"));
show(fn() => constant("\xff"));
show(fn() => ("\xff")());
show(fn() => new DateTimeZone("\xff"));
show(fn() => (new stdClass)->{"\xff"}());
show(fn() => Closure::fromCallable("\xff"));
show(fn() => new ("\xff"));
show(fn() => sprintf("%\xff", 1));
show(fn() => pack("\xff", 1));
echo error_get_last() === null ? "none\n" : "set\n";
restore_error_handler();
@preg_match("/a/\xfd", "a");
echo bin2hex(error_get_last()['message']), "\n";
--EXPECT--
E: 707265675f6d6174636828293a20556e6b6e6f776e206d6f6469666965722027ff27
E: 707265675f6d6174636828293a204e6f20656e64696e672064656c696d697465722027ff2720666f756e64
E: 646174655f64656661756c745f74696d657a6f6e655f73657428293a2054696d657a6f6e652049442027fe2720697320696e76616c6964
ValueError: 6d625f636f6e766572745f6b616e6128293a20417267756d656e742023322028246d6f64652920636f6e7461696e7320696e76616c696420666c61673a2027ff27
ValueError: 6d625f7374726c656e28293a20417267756d656e74202332202824656e636f64696e6729206d75737420626520612076616c696420656e636f64696e672c2022ff2220676976656e
ReflectionException: 436c6173732022ff2220646f6573206e6f74206578697374
Error: 556e646566696e656420636f6e7374616e742022ff22
Error: 43616c6c20746f20756e646566696e65642066756e6374696f6e20ff2829
DateInvalidTimeZoneException: 4461746554696d655a6f6e653a3a5f5f636f6e73747275637428293a20556e6b6e6f776e206f72206261642074696d657a6f6e652028ff29
Error: 43616c6c20746f20756e646566696e6564206d6574686f6420737464436c6173733a3aff2829
TypeError: 4661696c656420746f2063726561746520636c6f737572652066726f6d2063616c6c61626c653a2066756e6374696f6e2022ff22206e6f7420666f756e64206f7220696e76616c69642066756e6374696f6e206e616d65
Error: 436c6173732022ff22206e6f7420666f756e64
ValueError: 556e6b6e6f776e20666f726d6174207370656369666965722022ff22
ValueError: 5479706520ff3a20756e6b6e6f776e20666f726d617420636f6465
none
707265675f6d6174636828293a20556e6b6e6f776e206d6f6469666965722027fd27
