--TEST--
get_defined_constants(true) and ReflectionExtension::getConstants(): registration order
--FILE--
<?php
// constants of an extension come in its registration order
$all = get_defined_constants(true);
foreach (['json', 'pcre', 'date', 'tokenizer'] as $e) {
    $a = array_slice(array_keys($all[$e]), 0, 6);
    echo "$e: ", implode(' ', $a), "\n";
    $r = array_slice(array_keys((new ReflectionExtension($e))->getConstants()), 0, 6);
    echo "$e: ", implode(' ', $r), "\n";
}
$r = (new ReflectionExtension('dom'))->getConstants();
var_dump(isset($r['Dom\INDEX_SIZE_ERR']));
$r = (new ReflectionExtension('standard'))->getConstants();
var_dump(isset($r['STREAM_USE_PATH']));
?>
--EXPECT--
json: JSON_HEX_TAG JSON_HEX_AMP JSON_HEX_APOS JSON_HEX_QUOT JSON_FORCE_OBJECT JSON_NUMERIC_CHECK
json: JSON_HEX_TAG JSON_HEX_AMP JSON_HEX_APOS JSON_HEX_QUOT JSON_FORCE_OBJECT JSON_NUMERIC_CHECK
pcre: PREG_PATTERN_ORDER PREG_SET_ORDER PREG_OFFSET_CAPTURE PREG_UNMATCHED_AS_NULL PREG_SPLIT_NO_EMPTY PREG_SPLIT_DELIM_CAPTURE
pcre: PREG_PATTERN_ORDER PREG_SET_ORDER PREG_OFFSET_CAPTURE PREG_UNMATCHED_AS_NULL PREG_SPLIT_NO_EMPTY PREG_SPLIT_DELIM_CAPTURE
date: DATE_ATOM DATE_COOKIE DATE_ISO8601 DATE_ISO8601_EXPANDED DATE_RFC822 DATE_RFC850
date: DATE_ATOM DATE_COOKIE DATE_ISO8601 DATE_ISO8601_EXPANDED DATE_RFC822 DATE_RFC850
tokenizer: T_LNUMBER T_DNUMBER T_STRING T_NAME_FULLY_QUALIFIED T_NAME_RELATIVE T_NAME_QUALIFIED
tokenizer: T_LNUMBER T_DNUMBER T_STRING T_NAME_FULLY_QUALIFIED T_NAME_RELATIVE T_NAME_QUALIFIED
bool(true)
bool(true)
