--TEST--
iconv() //IGNORE: glibc's skip lengths for unmapped CJK sequences
--FILE--
<?php
// how many bytes //IGNORE skips for an unmapped double-byte sequence (glibc)
$cases = [
    "SJIS" => ["\x81\x7f", "\x85\x40", "\x85\xa1", "\xea\xa5", "\x81\x30"],
    "CP932" => ["\x84\x61", "\x84\xbf", "\x85\x40", "\x87\x5e", "\x87\xa1", "\x88\x40", "\xea\xa5", "\xeb\x40"],
    "UHC" => ["\x81\x41\x41", "\xa2\xe9", "\xc9\x41", "\xad\xa1"],
    "GBK" => ["\x81\x7f", "\xa1\x40", "\xfe\x50", "\xfe\xa1"],
    "GB2312" => ["\xaa\xa2", "\x8e\xa2", "\xa2\xa2"],
    "BIG5" => ["\xa3\xc0", "\xa3\x40", "\xc8\x40"],
    "EUC-JP-MS" => ["\xa2\xb0", "\xa9\xa1", "\x8f\xa1\xa1", "\xa0\xa1"],
];
foreach ($cases as $cs => $list) {
    foreach ($list as $s) {
        foreach (["\x40!", "\xa1\xa1!"] as $tail) {
            $r = @iconv($cs, "UTF-8//IGNORE", $s . $tail);
            echo $cs, " ", bin2hex($s . $tail), " ", $r === false ? "F" : bin2hex($r), "\n";
        }
    }
}
--EXPECT--
SJIS 817f4021 4021
SJIS 817fa1a121 efbda1efbda121
SJIS 85404021 4021
SJIS 8540a1a121 efbda1efbda121
SJIS 85a14021 4021
SJIS 85a1a1a121 efbda1efbda121
SJIS eaa54021 4021
SJIS eaa5a1a121 efbda1efbda121
SJIS 81304021 304021
SJIS 8130a1a121 30efbda1efbda121
CP932 84614021 4021
CP932 8461a1a121 efbda1efbda121
CP932 84bf4021 efbdbf4021
CP932 84bfa1a121 efbdbfefbda1efbda121
CP932 85404021 404021
CP932 8540a1a121 40efbda1efbda121
CP932 875e4021 4021
CP932 875ea1a121 efbda1efbda121
CP932 87a14021 efbda14021
CP932 87a1a1a121 efbda1efbda1efbda121
CP932 88404021 404021
CP932 8840a1a121 40efbda1efbda121
CP932 eaa54021 efbda54021
CP932 eaa5a1a121 efbda5efbda1efbda121
CP932 eb404021 404021
CP932 eb40a1a121 40efbda1efbda121
UHC 8141414021 eab082414021
UHC 814141a1a121 eab08241e3808021
UHC a2e94021 4021
UHC a2e9a1a121 e3808021
UHC c9414021 414021
UHC c941a1a121 41e3808021
UHC ada14021 4021
UHC ada1a1a121 e3808021
GBK 817f4021 4021
GBK 817fa1a121 e3808021
GBK a1404021 4021
GBK a140a1a121 e3808021
GBK fe504021 4021
GBK fe50a1a121 e3808021
GBK fea14021 21
GBK fea1a1a121 e3808021
GB2312 aaa24021 4021
GB2312 aaa2a1a121 e3808021
GB2312 8ea24021 4021
GB2312 8ea2a1a121 e3808021
GB2312 a2a24021 4021
GB2312 a2a2a1a121 e3808021
BIG5 a3c04021 4021
BIG5 a3c0a1a121 efb99b21
BIG5 a3404021 efbd974021
BIG5 a340a1a121 efbd97efb99b21
BIG5 c8404021 ef9eac4021
BIG5 c840a1a121 ef9eacefb99b21
EUC-JP-MS a2b04021 4021
EUC-JP-MS a2b0a1a121 e3808021
EUC-JP-MS a9a14021 4021
EUC-JP-MS a9a1a1a121 e3808021
EUC-JP-MS 8fa1a14021 4021
EUC-JP-MS 8fa1a1a1a121 e3808021
EUC-JP-MS a0a14021 4021
EUC-JP-MS a0a1a1a121 e3808021
