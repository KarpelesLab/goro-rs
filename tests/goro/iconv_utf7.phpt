--TEST--
iconv(): UTF-7, UTF-7-IMAP and UNICODE
--FILE--
<?php
function h($x) { return $x === false ? "FALSE" : bin2hex($x); }
foreach (["UNICODE", "UTF-7", "UTF7", "UTF-7-IMAP"] as $cs) {
    foreach (["a", "A+B", "a&b", "~\\!#", "é.", "éA", "日本", "x😀y", "a\tb\r\n", "€ €", "&-", "+-", "Hello, World!", "1 + 1 = 2", "\n\x7f&="] as $s) {
        $e = @iconv("UTF-8", $cs, $s);
        echo "$cs ", json_encode($s), " ", h($e), " ", h(@iconv($cs, "UTF-8", (string)$e)), "\n";
    }
}
foreach (["UNICODE" => ["\xff\xfea\x00", "\xfe\xff\x00a", "\x00a", "a", "\xff\xfe\x00\xd8"],
          "UTF-7" => ["+AGE-", "+AGEAYQ", "+", "+-", "+2D3eAA-", "+2D0-", "+3gA-", "~", "+AGF-", "+AG-", "+AGE.x", "&"],
          "UTF-7-IMAP" => ["&AGE-", "&-", "&", "+", "&ZeVnLA-", "\t", "~", "& ", "&~"]] as $cs => $list) {
    foreach ($list as $s) {
        foreach (["", "//IGNORE"] as $m) {
            error_clear_last();
            $d = @iconv($cs, "UTF-8$m", $s);
            echo "$cs$m ", bin2hex($s), " ", h($d), " ", error_get_last()['message'] ?? '', "\n";
        }
    }
}
var_dump(iconv_strlen("+AGEAYQ-", "UTF-7"), iconv_substr("+AGEAYQ-a", 1, 2, "UTF-7"), iconv_strpos("+AGEAYQ-a", "a", 0, "UTF-7"));
var_dump(iconv_mime_encode("S", "日本", ["output-charset" => "UTF-7", "scheme" => "Q"]));
--EXPECT--
UNICODE "a" fffe6100 61
UNICODE "A+B" fffe41002b004200 412b42
UNICODE "a&b" fffe610026006200 612662
UNICODE "~\\!#" fffe7e005c0021002300 7e5c2123
UNICODE "\u00e9." fffee9002e00 c3a92e
UNICODE "\u00e9A" fffee9004100 c3a941
UNICODE "\u65e5\u672c" fffee5652c67 e697a5e69cac
UNICODE "x\ud83d\ude00y" FALSE 
UNICODE "a\tb\r\n" fffe6100090062000d000a00 6109620d0a
UNICODE "\u20ac \u20ac" fffeac202000ac20 e282ac20e282ac
UNICODE "&-" fffe26002d00 262d
UNICODE "+-" fffe2b002d00 2b2d
UNICODE "Hello, World!" fffe480065006c006c006f002c00200057006f0072006c0064002100 48656c6c6f2c20576f726c6421
UNICODE "1 + 1 = 2" fffe310020002b002000310020003d0020003200 31202b2031203d2032
UNICODE "\n&=" fffe0a007f0026003d00 0a7f263d
UTF-7 "a" 61 61
UTF-7 "A+B" 412b2d42 412b42
UTF-7 "a&b" 612b4143592d62 612662
UTF-7 "~\\!#" 2b414834415841416841434d2d 7e5c2123
UTF-7 "\u00e9." 2b414f6b2e c3a92e
UTF-7 "\u00e9A" 2b414f6b2d41 c3a941
UTF-7 "\u65e5\u672c" 2b5a65566e4c412d e697a5e69cac
UTF-7 "x\ud83d\ude00y" 782b3244336541412d79 78f09f988079
UTF-7 "a\tb\r\n" 6109620d0a 6109620d0a
UTF-7 "\u20ac \u20ac" 2b494b77202b494b772d e282ac20e282ac
UTF-7 "&-" 2b4143592d2d 262d
UTF-7 "+-" 2b2d2d 2b2d
UTF-7 "Hello, World!" 48656c6c6f2c20576f726c642b4143452d 48656c6c6f2c20576f726c6421
UTF-7 "1 + 1 = 2" 31202b2d2031202b4144302032 31202b2031203d2032
UTF-7 "\n&=" 0a2b414838414a6741392d 0a7f263d
UTF7 "a" 61 61
UTF7 "A+B" 412b2d42 412b42
UTF7 "a&b" 612b4143592d62 612662
UTF7 "~\\!#" 2b414834415841416841434d2d 7e5c2123
UTF7 "\u00e9." 2b414f6b2e c3a92e
UTF7 "\u00e9A" 2b414f6b2d41 c3a941
UTF7 "\u65e5\u672c" 2b5a65566e4c412d e697a5e69cac
UTF7 "x\ud83d\ude00y" 782b3244336541412d79 78f09f988079
UTF7 "a\tb\r\n" 6109620d0a 6109620d0a
UTF7 "\u20ac \u20ac" 2b494b77202b494b772d e282ac20e282ac
UTF7 "&-" 2b4143592d2d 262d
UTF7 "+-" 2b2d2d 2b2d
UTF7 "Hello, World!" 48656c6c6f2c20576f726c642b4143452d 48656c6c6f2c20576f726c6421
UTF7 "1 + 1 = 2" 31202b2d2031202b4144302032 31202b2031203d2032
UTF7 "\n&=" 0a2b414838414a6741392d 0a7f263d
UTF-7-IMAP "a" 61 61
UTF-7-IMAP "A+B" 412b42 412b42
UTF-7-IMAP "a&b" 61262d62 612662
UTF-7-IMAP "~\\!#" 7e5c2123 7e5c2123
UTF-7-IMAP "\u00e9." 26414f6b2d2e c3a92e
UTF-7-IMAP "\u00e9A" 26414f6b2d41 c3a941
UTF-7-IMAP "\u65e5\u672c" 265a65566e4c412d e697a5e69cac
UTF-7-IMAP "x\ud83d\ude00y" 78263244336541412d79 78f09f988079
UTF-7-IMAP "a\tb\r\n" 612641416b2d62264141304143672d 6109620d0a
UTF-7-IMAP "\u20ac \u20ac" 26494b772d2026494b772d e282ac20e282ac
UTF-7-IMAP "&-" 262d2d 262d
UTF-7-IMAP "+-" 2b2d 2b2d
UTF-7-IMAP "Hello, World!" 48656c6c6f2c20576f726c6421 48656c6c6f2c20576f726c6421
UTF-7-IMAP "1 + 1 = 2" 31202b2031203d2032 31202b2031203d2032
UTF-7-IMAP "\n&=" 2641416f4166772d262d3d 0a7f263d
UNICODE fffe6100 61 
UNICODE//IGNORE fffe6100 61 
UNICODE feff0061 61 
UNICODE//IGNORE feff0061 61 
UNICODE 0061 e68480 
UNICODE//IGNORE 0061 e68480 
UNICODE 61 FALSE iconv(): Detected an incomplete multibyte character in input string
UNICODE//IGNORE 61 FALSE iconv(): Detected an incomplete multibyte character in input string
UNICODE fffe00d8 FALSE iconv(): Detected an illegal character in input string
UNICODE//IGNORE fffe00d8  
UTF-7 2b4147452d 61 
UTF-7//IGNORE 2b4147452d 61 
UTF-7 2b414745415951 6161 
UTF-7//IGNORE 2b414745415951 6161 
UTF-7 2b FALSE iconv(): Detected an incomplete multibyte character in input string
UTF-7//IGNORE 2b FALSE iconv(): Detected an incomplete multibyte character in input string
UTF-7 2b2d 2b 
UTF-7//IGNORE 2b2d 2b 
UTF-7 2b3244336541412d f09f9880 
UTF-7//IGNORE 2b3244336541412d f09f9880 
UTF-7 2b3244302d FALSE iconv(): Detected an illegal character in input string
UTF-7//IGNORE 2b3244302d  
UTF-7 2b3367412d FALSE iconv(): Detected an illegal character in input string
UTF-7//IGNORE 2b3367412d  
UTF-7 7e FALSE iconv(): Detected an illegal character in input string
UTF-7//IGNORE 7e  
UTF-7 2b4147462d FALSE iconv(): Detected an illegal character in input string
UTF-7//IGNORE 2b4147462d 61 
UTF-7 2b41472d FALSE iconv(): Detected an illegal character in input string
UTF-7//IGNORE 2b41472d  
UTF-7 2b4147452e78 612e78 
UTF-7//IGNORE 2b4147452e78 612e78 
UTF-7 26 26 
UTF-7//IGNORE 26 26 
UTF-7-IMAP 264147452d 61 
UTF-7-IMAP//IGNORE 264147452d 61 
UTF-7-IMAP 262d 26 
UTF-7-IMAP//IGNORE 262d 26 
UTF-7-IMAP 26 FALSE iconv(): Detected an incomplete multibyte character in input string
UTF-7-IMAP//IGNORE 26 FALSE iconv(): Detected an incomplete multibyte character in input string
UTF-7-IMAP 2b 2b 
UTF-7-IMAP//IGNORE 2b 2b 
UTF-7-IMAP 265a65566e4c412d e697a5e69cac 
UTF-7-IMAP//IGNORE 265a65566e4c412d e697a5e69cac 
UTF-7-IMAP 09 FALSE iconv(): Detected an illegal character in input string
UTF-7-IMAP//IGNORE 09  
UTF-7-IMAP 7e 7e 
UTF-7-IMAP//IGNORE 7e 7e 
UTF-7-IMAP 2620 FALSE iconv(): Detected an illegal character in input string
UTF-7-IMAP//IGNORE 2620  
UTF-7-IMAP 267e FALSE iconv(): Detected an illegal character in input string
UTF-7-IMAP//IGNORE 267e  
int(2)
string(2) "aa"
int(0)
string(23) "S: =?UTF-7?Q?+ZeVnLA-?="
