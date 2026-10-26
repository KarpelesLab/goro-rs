--TEST--
get_extension_funcs(): function table order
--FILE--
<?php
// The module's function table order (the stubs' declaration order)
echo implode(",", get_extension_funcs("date")), "\n";
echo implode(",", get_extension_funcs("json")), "\n";
echo implode(",", get_extension_funcs("ctype")), "\n";
--EXPECT--
strtotime,date,idate,gmdate,mktime,gmmktime,checkdate,strftime,gmstrftime,time,localtime,getdate,date_create,date_create_immutable,date_create_from_format,date_create_immutable_from_format,date_parse,date_parse_from_format,date_get_last_errors,date_format,date_modify,date_add,date_sub,date_timezone_get,date_timezone_set,date_offset_get,date_diff,date_time_set,date_date_set,date_isodate_set,date_timestamp_set,date_timestamp_get,timezone_open,timezone_name_get,timezone_name_from_abbr,timezone_offset_get,timezone_transitions_get,timezone_location_get,timezone_identifiers_list,timezone_abbreviations_list,timezone_version_get,date_interval_create_from_date_string,date_interval_format,date_default_timezone_set,date_default_timezone_get,date_sunrise,date_sunset,date_sun_info
json_encode,json_decode,json_validate,json_last_error,json_last_error_msg
ctype_alnum,ctype_alpha,ctype_cntrl,ctype_digit,ctype_lower,ctype_graph,ctype_print,ctype_punct,ctype_space,ctype_upper,ctype_xdigit
