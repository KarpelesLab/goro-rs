--TEST--
NumberFormatter::ORDINAL follows CLDR's %digits-ordinal rules (prefixes, sub-rule sets, root fallback)
--EXTENSIONS--
intl
--FILE--
<?php
// ja/zh/yue put 第 before the number (and before the minus), th/vi/id/fil
// a word or ke-; es/it/sv/bg/ca/uk go through gendered sub-rule sets whose
// -x rule gives the minus; locales without rules (hu, nb, de) get root's
// "=#,##0=."; ru and el have plain digits
foreach (["en", "fr", "de", "hu", "nb", "ja", "zh", "zh_Hant", "yue", "ko", "th", "vi", "id", "fil", "es", "es_MX", "it", "pt", "sv", "nl", "ru", "uk", "bg", "ca", "el"] as $l) {
    $f = new NumberFormatter($l, NumberFormatter::ORDINAL);
    $out = [];
    foreach ([1, 2, 3, 4, 11, 21, 1000, -5, 0] as $n) {
        $out[] = $f->format($n);
    }
    echo str_pad($l, 8), implode(" ", $out), "\n";
}
// spelled-out fractions: ja uses ・ with no spaces, uk кома
foreach (["ja", "uk", "en"] as $l) {
    $f = new NumberFormatter($l, NumberFormatter::SPELLOUT);
    echo $l, ": ", $f->format(12.25), " | ", $f->format(-0.5), "\n";
}
?>
--EXPECT--
en      1st 2nd 3rd 4th 11th 21st 1,000th −5th 0th
fr      1er 2e 3e 4e 11e 21e 1 000e −5e 0e
de      1. 2. 3. 4. 11. 21. 1.000. −5. 0.
hu      1. 2. 3. 4. 11. 21. 1 000. −5. 0.
nb      1. 2. 3. 4. 11. 21. 1 000. −5. 0.
ja      第1 第2 第3 第4 第11 第21 第1,000 第−5 第0
zh      第1 第2 第3 第4 第11 第21 第1,000 第−5 第0
zh_Hant 第1 第2 第3 第4 第11 第21 第1,000 第−5 第0
yue     第1 第2 第3 第4 第11 第21 第1,000 第−5 第0
ko      1번째 2번째 3번째 4번째 11번째 21번째 1,000번째 −5번째 0번째
th      ที่ 1 ที่ 2 ที่ 3 ที่ 4 ที่ 11 ที่ 21 ที่ 1,000 ที่ −5 ที่ 0
vi      thứ 1 thứ 2 thứ 3 thứ 4 thứ 11 thứ 21 thứ 1.000 −thứ 5 thứ 0
id      ke-1 ke-2 ke-3 ke-4 ke-11 ke-21 ke-1.000 −ke-5 ke-0
fil     ika1 ika2 ika3 ika4 ika11 ika21 ika1,000 −ika5 ika0
es      1.º 2.º 3.º 4.º 11.º 21.º 1.000.º −5.º 0.º
es_MX   1º. 2º. 3º. 4º. 11º. 21º. 1,000º. −5º. 0º.
it      1º 2º 3º 4º 11º 21º 1.000º −5º 0º
pt      1º 2º 3º 4º 11º 21º 1.000º −5º 0º
sv      1:a 2:a 3:e 4:e 11:e 21:a 1 000:e −5:e 0:e
nl      1e 2e 3e 4e 11e 21e 1.000e −5e 0e
ru      1 2 3 4 11 21 1 000 −5 0
uk      1-ий 2-ий 3-ій 4-ий 11-ий 21-ий 1 000-ий −5-ий 0-ий
bg      1-ви 2-ри 3-ти 4-ти 11-и 21-ви 1 000-и −5-и 0-и
ca      1r 2n 3r 4t 11è 21è 1.000è −5è 0è
el      1 2 3 4 11 21 1.000 −5 0
ja: 十二・二五 | マイナス〇・五
uk: дванадцять кома два пʼять | мінус нуль кома пʼять
en: twelve point two five | minus zero point five
