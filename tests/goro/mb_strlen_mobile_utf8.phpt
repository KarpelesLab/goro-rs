--TEST--
mb_strlen() in the mobile UTF-8 variants decodes instead of counting bytes
--FILE--
<?php
// mobile UTF-8 variants decode some emoji to two characters
foreach (["UTF-8-Mobile#DOCOMO", "UTF-8-Mobile#KDDI-A", "UTF-8-Mobile#KDDI-B", "UTF-8-Mobile#SOFTBANK"] as $e) {
    foreach (["#\u{20E3}", "\u{1F1EF}\u{1F1F5}", "1\u{20E3}x", "abc"] as $s) {
        $c = mb_convert_encoding($s, $e, "UTF-8");
        echo $e, " ", bin2hex($c), " ", mb_strlen($c, $e), " ", bin2hex(mb_convert_encoding($c, "UTF-8", $e)), "\n";
    }
    var_dump(mb_strlen("\xee\x82\x9b\xf3\xbe\xa0\xac", $e));
}
--EXPECT--
UTF-8-Mobile#DOCOMO ee9ba0 2 23e283a3
UTF-8-Mobile#DOCOMO f09f87aff09f87b5 2 f09f87aff09f87b5
UTF-8-Mobile#DOCOMO ee9ba278 3 31e283a378
UTF-8-Mobile#DOCOMO 616263 3 616263
int(2)
UTF-8-Mobile#KDDI-A eeae84 2 23e283a3
UTF-8-Mobile#KDDI-A ee938c 2 f09f87aff09f87b5
UTF-8-Mobile#KDDI-A ee94a278 3 31e283a378
UTF-8-Mobile#KDDI-A 616263 3 616263
int(2)
UTF-8-Mobile#KDDI-B eeb689 2 23e283a3
UTF-8-Mobile#KDDI-B eebea5 2 f09f87aff09f87b5
UTF-8-Mobile#KDDI-B eebfbb78 3 31e283a378
UTF-8-Mobile#KDDI-B 616263 3 616263
int(2)
UTF-8-Mobile#SOFTBANK ee8890 2 23e283a3
UTF-8-Mobile#SOFTBANK ee948b 2 f09f87aff09f87b5
UTF-8-Mobile#SOFTBANK ee889c78 3 31e283a378
UTF-8-Mobile#SOFTBANK 616263 3 616263
int(2)
