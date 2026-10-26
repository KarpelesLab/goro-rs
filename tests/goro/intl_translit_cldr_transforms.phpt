--TEST--
Transliterator: CLDR transforms (Han, Arabic, Hebrew, Indic, Thai... to Latin, their reverses) and Any-Latin over them
--EXTENSIONS--
intl
--FILE--
<?php
// the transforms ICU builds from CLDR's rules, registered under their
// aliases (Latin-Cyrillic is the reverse of Cyrillic-Latin, Thaana-Latin
// of Latin-Thaana); Any-Latin romanizes each script run with its
// "<Script>-Latin" transform
$samples = ["中文 北京 你好", "مرحبا", "שלום", "नमस्ते", "தமிழ்", "বাংলা", "ગુજરાતી", "ไทย สวัสดี", "ދިވެހި", "ܣܘܪܝܝܐ", "Привет", "Ελληνικά", "한국어", "ひらがな"];
foreach (["Any-Latin", "Any-Latin; Latin-ASCII; Lower()"] as $id) {
    $t = Transliterator::create($id);
    foreach ($samples as $s) {
        echo $id, ": ", $s, " => ", $t->transliterate($s), "\n";
    }
}
foreach ([
    ["Han-Latin", "重庆银行"], ["Hans-Latn", "汉语"], ["Arabic-Latin", "السلام عليكم"], ["Hebrew-Latin", "עולם"],
    ["Devanagari-Latin", "हिन्दी"], ["Thai-Latin", "ภาษาไทย"], ["Latin-Cyrillic", "Moskva"], ["Latin-Greek", "Athēnai"],
    ["Latin-Arabic", "salam"], ["Latin-Hebrew", "shalom"], ["Thaana-Latin", "ދިވެހި"], ["de-ASCII", "Müller Größe"],
    ["Fullwidth-Halfwidth", "ＡＢＣ１２３"], ["Halfwidth-Fullwidth", "ABC ｶﾀｶﾅ"], ["Any-Publishing", "\"quoted\" -- it's ('a')"],
    ["Latin-Hangul", "hangug-eo"],
] as [$id, $s]) {
    $t = Transliterator::create($id);
    echo $id, ": ", $t ? $t->transliterate($s) : "NULL", "\n";
}
var_dump(Transliterator::create("Han-Latin/Names"));
?>
--EXPECT--
Any-Latin: 中文 北京 你好 => zhōng wén běi jīng nǐ hǎo
Any-Latin: مرحبا => mrḥbạ
Any-Latin: שלום => şlwm
Any-Latin: नमस्ते => namastē
Any-Latin: தமிழ் => tamiḻ
Any-Latin: বাংলা => bānlā
Any-Latin: ગુજરાતી => gujarātī
Any-Latin: ไทย สวัสดี => thịy s̄wạs̄dī
Any-Latin: ދިވެހި => divehi
Any-Latin: ܣܘܪܝܝܐ => swryyʾ
Any-Latin: Привет => Privet
Any-Latin: Ελληνικά => Ellēniká
Any-Latin: 한국어 => hangug-eo
Any-Latin: ひらがな => hiragana
Any-Latin; Latin-ASCII; Lower(): 中文 北京 你好 => zhong wen bei jing ni hao
Any-Latin; Latin-ASCII; Lower(): مرحبا => mrhba
Any-Latin; Latin-ASCII; Lower(): שלום => slwm
Any-Latin; Latin-ASCII; Lower(): नमस्ते => namaste
Any-Latin; Latin-ASCII; Lower(): தமிழ் => tamil
Any-Latin; Latin-ASCII; Lower(): বাংলা => banla
Any-Latin; Latin-ASCII; Lower(): ગુજરાતી => gujarati
Any-Latin; Latin-ASCII; Lower(): ไทย สวัสดี => thiy swasdi
Any-Latin; Latin-ASCII; Lower(): ދިވެހި => divehi
Any-Latin; Latin-ASCII; Lower(): ܣܘܪܝܝܐ => swryyʾ
Any-Latin; Latin-ASCII; Lower(): Привет => privet
Any-Latin; Latin-ASCII; Lower(): Ελληνικά => ellenika
Any-Latin; Latin-ASCII; Lower(): 한국어 => hangug-eo
Any-Latin; Latin-ASCII; Lower(): ひらがな => hiragana
Han-Latin: chóng qìng yín xíng
Hans-Latn: hàn yǔ
Arabic-Latin: ạlslạm ʿlykm
Hebrew-Latin: ʻwlm
Devanagari-Latin: hindī
Thai-Latin: p̣hās̄ʹā thịy
Latin-Cyrillic: Москва
Latin-Greek: Ἀθηναι
Latin-Arabic: سَلَم
Latin-Hebrew: סהַלֳם
Thaana-Latin: divehi
de-ASCII: Mueller Groesse
Fullwidth-Halfwidth: ABC123
Halfwidth-Fullwidth: ＡＢＣ　カタカナ
Any-Publishing: “quoted” — it’s (‘a’)
Latin-Hangul: 한국어
NULL
