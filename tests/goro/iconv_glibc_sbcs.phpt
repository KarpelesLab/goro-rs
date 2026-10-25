--TEST--
iconv(): glibc's single-byte charsets and aliases
--FILE--
<?php
$probe = "Aé€ñЖ日ąčαש";
foreach (["CP1250", "WINDOWS-1250", "CP1253", "CP1256", "CP1257", "CP874", "TIS-620", "MACINTOSH", "MAC", "CP437", "IBM437", "CP852", "CP855", "CP866", "KOI8-RU", "PT154", "ISO-IR-100", "L2", "CSISOLATIN2", "ISO-8859-11", "IBM1047", "CP037", "MACCYRILLIC", "ISO-IR-6", "CP1252", "ISO-8859-7", "ISO-8859-8", "HP-ROMAN8"] as $n) {
    $r = iconv("UTF-8", "$n//IGNORE", $probe);
    echo $n, ": ", $r === false ? "FALSE" : bin2hex($r);
    if ($r !== false) {
        echo " back=", iconv($n, "UTF-8", $r);
    }
    echo "\n";
}
// undefined bytes
var_dump(@iconv("CP1253", "UTF-8", "a\xaab"));
var_dump(iconv("CP1253", "UTF-8//IGNORE", "a\xaab"));
var_dump(iconv("CP437", "UTF-8", "\x80\x9b\xe1\xfe"));
var_dump(bin2hex(iconv("UTF-8", "CP437", "ßΣ")));
--EXPECT--
CP1250: 41e980b9e8 back=Aé€ąč
WINDOWS-1250: 41e980b9e8 back=Aé€ąč
CP1253: 4180e1 back=A€α
CP1256: 41e980 back=Aé€
CP1257: 41e980e0e8 back=Aé€ąč
CP874: 4180 back=A€
TIS-620: 41 back=A
MACINTOSH: 418edb96 back=Aé€ñ
MAC: 418edb96 back=Aé€ñ
CP437: 4182a4e0 back=Aéñα
IBM437: 4182a4e0 back=Aéñα
CP852: 4182a59f back=Aéąč
CP855: 41ea back=AЖ
CP866: 4186 back=AЖ
KOI8-RU: 41f6 back=AЖ
PT154: 41c6 back=AЖ
ISO-IR-100: 41e9f1 back=Aéñ
L2: 41e9b1e8 back=Aéąč
CSISOLATIN2: 41e9b1e8 back=Aéąč
ISO-8859-11: 41 back=A
IBM1047: c15149 back=Aéñ
CP037: c15149 back=Aéñ
MACCYRILLIC: 4186 back=AЖ
ISO-IR-6: 41 back=A
CP1252: 41e980f1 back=Aé€ñ
ISO-8859-7: 41a4e1 back=A€α
ISO-8859-8: 41f9 back=Aש
HP-ROMAN8: 41c5b7 back=Aéñ
bool(false)
string(2) "ab"
string(9) "Ç¢ß■"
string(4) "e1e4"
