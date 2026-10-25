--TEST--
iconv() and convert.iconv.*: where php's output buffer was full
--FILE--
<?php
// php's iconv() buffer (the input's length + 32, grown by the input's
// length) and the stream filter's (the bucket's length, doubled): glibc's
// stateful encoders write the escape sequences of a character without
// room, so where the buffer was full shows
function filt($cs, $u8, $from = "UTF-8") {
    $fp = fopen("php://memory", "w+");
    $f = stream_filter_append($fp, "convert.iconv.$from/$cs", STREAM_FILTER_WRITE);
    fwrite($fp, $u8);
    stream_filter_remove($f);
    rewind($fp);
    $r = stream_get_contents($fp);
    fclose($fp);
    return bin2hex($r);
}
// a new line forgets the designations; the SS2 written before the
// character without room comes again
echo bin2hex(iconv("UTF-8", "ISO-2022-CN-EXT", hex2bin("e49883e9be83e39080e7b697f0a08284e792b30ae5aca7"))), "\n";
foreach (["UTF-16BE"] as $from) {
    for ($k = 0; $k < 8; $k++) {
        $s = mb_convert_encoding(str_repeat("a", $k) . "紅唹乂乂中㐀\u{4E0F}", $from, "UTF-8");
        echo $k, " ", filt("ISO-2022-CN-EXT", $s, $from), " ", filt("ISO-2022-JP-2", mb_convert_encoding(str_repeat("a", $k) . "日本\u{D55C}\u{E9}\u{AB}", $from, "UTF-8"), $from), "\n";
    }
}
echo filt("ISO-2022-KR//TRANSLIT", "Ê"), " ", filt("ISO-2022-KR//TRANSLIT", "aaaaÊ"), " ", filt("ISO-2022-JP//TRANSLIT", "日日é"), "\n";
echo filt("UTF-16BE", "中文字"), "\n";
--EXPECT--
1b242b4a1b4f67301b24294176341b242b4c1b4f222c1b242b491b4f43681b242b4c1b4f50501b242b491b4f554d0f0a1b242a481b4e1b4e5c5f0f
0 1b2429470e52781b242a481b4e1b4e34282121212144631b242b4c1b4f222c1b4e212d0f 1b2442467c4b5c1b24284347511b2428442b311b2e411b4e2b1b2842
1 611b2429470e52781b242a481b4e34282121212144631b242b4c1b4f222c1b4e1b4e212d0f 611b2442467c4b5c1b24284347511b2428442b311b2e411b4e2b1b2842
2 61611b2429470e52781b242a481b4e34282121212144631b242b4c1b4f222c1b4e212d0f 61611b2442467c4b5c1b24284347511b2428442b311b2e411b4e2b1b2842
3 6161611b2429470e52781b242a481b4e34282121212144631b242b4c1b4f222c1b4e212d0f 6161611b2442467c4b5c1b24284347511b2428442b311b2e411b4e2b1b2842
4 616161611b2429470e52781b242a481b4e34282121212144631b242b4c1b4f222c1b4e212d0f 616161611b2442467c4b5c1b24284347511b2428442b311b2e411b4e2b1b2842
5 61616161611b2429470e52781b242a481b4e34282121212144631b242b4c1b4f222c1b4e212d0f 61616161611b2442467c4b5c1b24284347511b2428442b311b2e411b4e2b1b2842
6 6161616161611b2429470e52781b242a481b4e34282121212144631b242b4c1b4f222c1b4e212d0f 6161616161611b2442467c4b5c1b24284347511b2428442b311b2e411b4e2b1b2842
7 616161616161611b2429470e52781b242a481b4e34282121212144631b242b4c1b4f222c1b4e212d0f 616161616161611b2442467c4b5c1b24284347511b2428442b311b2e411b4e2b1b2842
1b24294345 1b2429436161616145 1b2442467c467c1b284265
4e2d65875b57
