--TEST--
iconv: CP1255 / CP1258 / TCVN hold a character back for a combining one (iconv_strlen/substr/strpos, //IGNORE flush)
--FILE--
<?php
// glibc's decoders keep a character that may take a combining mark (a
// Latin letter in CP1258/TCVN, a Hebrew letter in CP1255) until the next
// byte or the flush. php's character loops see it late: an input that is
// just such a character has no length, iconv_substr()'s flush after the
// last character taken writes it too, and iconv() //IGNORE fails when the
// held character cannot be written at the flush
foreach (["CP1258" => ["a", "ab", "abc", "\xc3", "1", "a\xec", "a\xecb"], "TCVN" => ["\x80", "\xc3", "\xc3\x80", "\xc3\x80\xef", "\xfc\xc3\x80\xef\xac"], "CP1255" => ["\xe0", "A", "\xf9\xcc\xd1", "\xe0\xe1"]] as $cs => $list) {
    foreach ($list as $s) {
        echo $cs, " ", bin2hex($s), " len=", var_export(iconv_strlen($s, $cs), true);
        foreach ([[0, 1], [0, 2], [1, 1], [1, 2], [-1, 1]] as [$o, $l]) {
            echo " sub($o,$l)=", bin2hex((string)iconv_substr($s, $o, $l, $cs));
        }
        echo " pos=", var_export(iconv_strpos($s . "x", "x", 0, $cs), true), "\n";
    }
}
var_dump(iconv("CP1255", "BIG5//IGNORE", "\xe0\xf0"));
var_dump(iconv("CP1255", "BIG5//IGNORE", "\xe0A"));
var_dump(bin2hex(iconv("CP1258", "ASCII//TRANSLIT", "Vi\xd2\xeat")));
?>
--EXPECTF--
CP1258 61 len=0 sub(0,1)= sub(0,2)= sub(1,1)= sub(1,2)= sub(-1,1)= pos=1
CP1258 6162 len=2 sub(0,1)=6162 sub(0,2)=6162 sub(1,1)=62 sub(1,2)=62 sub(-1,1)=62 pos=2
CP1258 616263 len=3 sub(0,1)=6162 sub(0,2)=616263 sub(1,1)=6263 sub(1,2)=6263 sub(-1,1)=63 pos=3
CP1258 c3 len=0 sub(0,1)= sub(0,2)= sub(1,1)= sub(1,2)= sub(-1,1)= pos=1
CP1258 31 len=1 sub(0,1)=31 sub(0,2)=31 sub(1,1)= sub(1,2)= sub(-1,1)=31 pos=1
CP1258 61ec len=1 sub(0,1)=e1 sub(0,2)=e1 sub(1,1)= sub(1,2)= sub(-1,1)=e1 pos=1
CP1258 61ec62 len=2 sub(0,1)=e162 sub(0,2)=e162 sub(1,1)=62 sub(1,2)=62 sub(-1,1)=62 pos=2
TCVN 80 len=0 sub(0,1)= sub(0,2)= sub(1,1)= sub(1,2)= sub(-1,1)= pos=1
TCVN c3 len=1 sub(0,1)=c3 sub(0,2)=c3 sub(1,1)= sub(1,2)= sub(-1,1)=c3 pos=1
TCVN c380 len=2 sub(0,1)=c380 sub(0,2)=c380 sub(1,1)=80 sub(1,2)=80 sub(-1,1)=80 pos=2
TCVN c380ef len=3 sub(0,1)=c3 sub(0,2)=c380ef sub(1,1)=80ef sub(1,2)=80ef sub(-1,1)=ef pos=3
TCVN fcc380efac len=5 sub(0,1)=fc sub(0,2)=fcc3 sub(1,1)=c3 sub(1,2)=c380ef sub(-1,1)=ac pos=5
CP1255 e0 len=0 sub(0,1)= sub(0,2)= sub(1,1)= sub(1,2)= sub(-1,1)= pos=1
CP1255 41 len=1 sub(0,1)=41 sub(0,2)=41 sub(1,1)= sub(1,2)= sub(-1,1)=41 pos=1
CP1255 f9ccd1 len=1 sub(0,1)=f9ccd1 sub(0,2)=f9ccd1 sub(1,1)= sub(1,2)= sub(-1,1)=f9ccd1 pos=1
CP1255 e0e1 len=2 sub(0,1)=e0e1 sub(0,2)=e0e1 sub(1,1)=e1 sub(1,2)=e1 sub(-1,1)=e1 pos=2

Notice: iconv(): Detected an illegal character in input string in %s on line %d
bool(false)
string(1) "A"
string(8) "56696574"
