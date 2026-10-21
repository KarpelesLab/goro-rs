//! PHP ext/date for goro-rs: a port of timelib + php_date.c.

pub mod timelib;

mod classes;
mod format;
mod funcs;
mod state;
mod zpp;

use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, Vm, VmError};

/// Run a builtin with a call-stack frame pushed, so exceptions thrown from it
/// carry the function in their trace (as internal functions do in PHP).
fn framed(vm: &mut Vm, name: &str, args: &[Value], f: fn(&mut Vm, &[Value]) -> Result<Value, VmError>) -> Result<Value, VmError> {
    let line = vm.current_line;
    let file = vm.current_file.clone();
    vm.call_stack.push((name.to_string(), file, line, args.to_vec(), false));
    let r = f(vm, args);
    vm.call_stack.pop();
    r
}

macro_rules! wrap {
    ($w:ident, $name:literal, $f:path) => {
        pub fn $w(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
            framed(vm, $name, args, $f)
        }
    };
}

mod wrapped {
    use super::*;
    use crate::classes::*;
    use crate::funcs::*;
    wrap!(w_date, "date", date_fn);
    wrap!(w_gmdate, "gmdate", gmdate_fn);
    wrap!(w_idate, "idate", idate_fn);
    wrap!(w_strtotime, "strtotime", strtotime_fn);
    wrap!(w_mktime, "mktime", mktime_fn);
    wrap!(w_gmmktime, "gmmktime", gmmktime_fn);
    wrap!(w_checkdate, "checkdate", checkdate_fn);
    wrap!(w_strftime, "strftime", strftime_fn);
    wrap!(w_gmstrftime, "gmstrftime", gmstrftime_fn);
    wrap!(w_time, "time", time_fn);
    wrap!(w_localtime, "localtime", localtime_fn);
    wrap!(w_getdate, "getdate", getdate_fn);
    wrap!(w_microtime, "microtime", microtime_fn);
    wrap!(w_gettimeofday, "gettimeofday", gettimeofday_fn);
    wrap!(w_date_create, "date_create", date_create_fn);
    wrap!(w_date_create_immutable, "date_create_immutable", date_create_immutable_fn);
    wrap!(w_date_create_from_format, "date_create_from_format", date_create_from_format_fn);
    wrap!(w_date_create_immutable_from_format, "date_create_immutable_from_format", date_create_immutable_from_format_fn);
    wrap!(w_date_parse, "date_parse", date_parse_fn);
    wrap!(w_date_parse_from_format, "date_parse_from_format", date_parse_from_format_fn);
    wrap!(w_date_get_last_errors, "date_get_last_errors", date_get_last_errors_fn);
    wrap!(w_date_format, "date_format", date_format_fn);
    wrap!(w_date_modify, "date_modify", date_modify_fn);
    wrap!(w_date_add, "date_add", date_add_fn);
    wrap!(w_date_sub, "date_sub", date_sub_fn);
    wrap!(w_date_timezone_get, "date_timezone_get", date_timezone_get_fn);
    wrap!(w_date_timezone_set, "date_timezone_set", date_timezone_set_fn);
    wrap!(w_date_offset_get, "date_offset_get", date_offset_get_fn);
    wrap!(w_date_diff, "date_diff", date_diff_fn);
    wrap!(w_date_time_set, "date_time_set", date_time_set_fn);
    wrap!(w_date_date_set, "date_date_set", date_date_set_fn);
    wrap!(w_date_isodate_set, "date_isodate_set", date_isodate_set_fn);
    wrap!(w_date_timestamp_set, "date_timestamp_set", date_timestamp_set_fn);
    wrap!(w_date_timestamp_get, "date_timestamp_get", date_timestamp_get_fn);
    wrap!(w_timezone_open, "timezone_open", timezone_open_fn);
    wrap!(w_timezone_name_get, "timezone_name_get", timezone_name_get_fn);
    wrap!(w_timezone_name_from_abbr, "timezone_name_from_abbr", timezone_name_from_abbr_fn);
    wrap!(w_timezone_offset_get, "timezone_offset_get", timezone_offset_get_fn);
    wrap!(w_timezone_transitions_get, "timezone_transitions_get", timezone_transitions_get_fn);
    wrap!(w_timezone_location_get, "timezone_location_get", timezone_location_get_fn);
    wrap!(w_timezone_identifiers_list, "timezone_identifiers_list", timezone_identifiers_list_fn);
    wrap!(w_timezone_abbreviations_list, "timezone_abbreviations_list", timezone_abbreviations_list_fn);
    wrap!(w_timezone_version_get, "timezone_version_get", timezone_version_get_fn);
    wrap!(w_date_interval_create_from_date_string, "date_interval_create_from_date_string", date_interval_create_from_date_string_fn);
    wrap!(w_date_interval_format, "date_interval_format", date_interval_format_fn);
    wrap!(w_date_default_timezone_set, "date_default_timezone_set", date_default_timezone_set_fn);
    wrap!(w_date_default_timezone_get, "date_default_timezone_get", date_default_timezone_get_fn);
    wrap!(w_date_sunrise, "date_sunrise", date_sunrise_fn);
    wrap!(w_date_sunset, "date_sunset", date_sunset_fn);
    wrap!(w_date_sun_info, "date_sun_info", date_sun_info_fn);
}

pub fn register(vm: &mut Vm) {
    use classes::*;
    use wrapped::*;
    vm.register_extension(b"date");

    let f: &[(&[u8], goro_core::vm::BuiltinFn, &[&[u8]])] = &[
        (b"date", w_date, &[b"format", b"timestamp"]),
        (b"gmdate", w_gmdate, &[b"format", b"timestamp"]),
        (b"idate", w_idate, &[b"format", b"timestamp"]),
        (b"strtotime", w_strtotime, &[b"datetime", b"baseTimestamp"]),
        (b"mktime", w_mktime, &[b"hour", b"minute", b"second", b"month", b"day", b"year"]),
        (b"gmmktime", w_gmmktime, &[b"hour", b"minute", b"second", b"month", b"day", b"year"]),
        (b"checkdate", w_checkdate, &[b"month", b"day", b"year"]),
        (b"strftime", w_strftime, &[b"format", b"timestamp"]),
        (b"gmstrftime", w_gmstrftime, &[b"format", b"timestamp"]),
        (b"time", w_time, &[]),
        (b"localtime", w_localtime, &[b"timestamp", b"associative"]),
        (b"getdate", w_getdate, &[b"timestamp"]),
        (b"microtime", w_microtime, &[b"as_float"]),
        (b"gettimeofday", w_gettimeofday, &[b"as_float"]),
        (b"date_create", w_date_create, &[b"datetime", b"timezone"]),
        (b"date_create_immutable", w_date_create_immutable, &[b"datetime", b"timezone"]),
        (b"date_create_from_format", w_date_create_from_format, &[b"format", b"datetime", b"timezone"]),
        (b"date_create_immutable_from_format", w_date_create_immutable_from_format, &[b"format", b"datetime", b"timezone"]),
        (b"date_parse", w_date_parse, &[b"datetime"]),
        (b"date_parse_from_format", w_date_parse_from_format, &[b"format", b"datetime"]),
        (b"date_get_last_errors", w_date_get_last_errors, &[]),
        (b"date_format", w_date_format, &[b"object", b"format"]),
        (b"date_modify", w_date_modify, &[b"object", b"modifier"]),
        (b"date_add", w_date_add, &[b"object", b"interval"]),
        (b"date_sub", w_date_sub, &[b"object", b"interval"]),
        (b"date_timezone_get", w_date_timezone_get, &[b"object"]),
        (b"date_timezone_set", w_date_timezone_set, &[b"object", b"timezone"]),
        (b"date_offset_get", w_date_offset_get, &[b"object"]),
        (b"date_diff", w_date_diff, &[b"baseObject", b"targetObject", b"absolute"]),
        (b"date_time_set", w_date_time_set, &[b"object", b"hour", b"minute", b"second", b"microsecond"]),
        (b"date_date_set", w_date_date_set, &[b"object", b"year", b"month", b"day"]),
        (b"date_isodate_set", w_date_isodate_set, &[b"object", b"year", b"week", b"dayOfWeek"]),
        (b"date_timestamp_set", w_date_timestamp_set, &[b"object", b"timestamp"]),
        (b"date_timestamp_get", w_date_timestamp_get, &[b"object"]),
        (b"timezone_open", w_timezone_open, &[b"timezone"]),
        (b"timezone_name_get", w_timezone_name_get, &[b"object"]),
        (b"timezone_name_from_abbr", w_timezone_name_from_abbr, &[b"abbr", b"utcOffset", b"isDST"]),
        (b"timezone_offset_get", w_timezone_offset_get, &[b"object", b"datetime"]),
        (b"timezone_transitions_get", w_timezone_transitions_get, &[b"object", b"timestampBegin", b"timestampEnd"]),
        (b"timezone_location_get", w_timezone_location_get, &[b"object"]),
        (b"timezone_identifiers_list", w_timezone_identifiers_list, &[b"timezoneGroup", b"countryCode"]),
        (b"timezone_abbreviations_list", w_timezone_abbreviations_list, &[]),
        (b"timezone_version_get", w_timezone_version_get, &[]),
        (b"date_interval_create_from_date_string", w_date_interval_create_from_date_string, &[b"datetime"]),
        (b"date_interval_format", w_date_interval_format, &[b"object", b"format"]),
        (b"date_default_timezone_set", w_date_default_timezone_set, &[b"timezoneId"]),
        (b"date_default_timezone_get", w_date_default_timezone_get, &[]),
        (b"date_sunrise", w_date_sunrise, &[b"timestamp", b"returnFormat", b"latitude", b"longitude", b"zenith", b"utcOffset"]),
        (b"date_sunset", w_date_sunset, &[b"timestamp", b"returnFormat", b"latitude", b"longitude", b"zenith", b"utcOffset"]),
        (b"date_sun_info", w_date_sun_info, &[b"timestamp", b"latitude", b"longitude"]),
    ];
    for (name, func, params) in f {
        vm.register_function_with_params(name, *func, params);
    }

    let h = NativeClassHandler { has_method, call: call_method, prop_added: Some(state::prop_added), prop_write: Some(state::prop_write), compare_check: Some(state::compare_check) };
    for c in [&b"datetime"[..], b"datetimeimmutable", b"datetimezone", b"dateinterval", b"dateperiod", b"internaliterator"] {
        vm.native_class_handlers.insert(c.to_vec(), h);
    }
    vm.ini_validators.insert(b"date.timezone".to_vec(), funcs::validate_date_timezone);
    vm.object_construct_handlers.insert(b"datetime".to_vec(), construct_datetime);
    vm.object_construct_handlers.insert(b"datetimeimmutable".to_vec(), construct_datetime);
    vm.object_construct_handlers.insert(b"datetimezone".to_vec(), construct_timezone);
    vm.object_construct_handlers.insert(b"dateinterval".to_vec(), construct_interval);
    vm.object_construct_handlers.insert(b"dateperiod".to_vec(), construct_period);
}
