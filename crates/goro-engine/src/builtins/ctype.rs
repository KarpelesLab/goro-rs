//! ext/ctype (PHP manual: Ctype functions): character classes of the C
//! locale. Non-string arguments are deprecated: ints from -128 to 255 are
//! one character (negative ones plus 256), other ints their decimal
//! digits, anything else false (observed on PHP 8.5).

use goro_runtime::Value;

use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "ctype",
    functions: &[
        ("function ctype_alnum(mixed $text): bool", ctype_alnum),
        ("function ctype_alpha(mixed $text): bool", ctype_alpha),
        ("function ctype_cntrl(mixed $text): bool", ctype_cntrl),
        ("function ctype_digit(mixed $text): bool", ctype_digit),
        ("function ctype_graph(mixed $text): bool", ctype_graph),
        ("function ctype_lower(mixed $text): bool", ctype_lower),
        ("function ctype_print(mixed $text): bool", ctype_print),
        ("function ctype_punct(mixed $text): bool", ctype_punct),
        ("function ctype_space(mixed $text): bool", ctype_space),
        ("function ctype_upper(mixed $text): bool", ctype_upper),
        ("function ctype_xdigit(mixed $text): bool", ctype_xdigit),
    ],
    listed: true,
    ..Module::EMPTY
};

fn check(vm: &mut Vm, a: &Args, f: fn(u8) -> bool) -> R<Value> {
    let v = a.val(0);
    match &v {
        Value::Str(s) => Ok(Value::Bool(!s.is_empty() && s.iter().all(|&c| f(c)))),
        other => {
            vm.fdeprecated(
                a,
                &format!(
                    "Argument of type {} will be interpreted as string in the future",
                    other.type_name_for_error()
                ),
            )?;
            Ok(Value::Bool(match other {
                Value::Int(i) if (-128..=255).contains(i) => {
                    let c = if *i < 0 { *i + 256 } else { *i };
                    f(c as u8)
                }
                Value::Int(i) => i.to_string().bytes().all(f),
                _ => false,
            }))
        }
    }
}

macro_rules! ctypes {
    ($($name:ident => $f:expr),* $(,)?) => {
        $(
            fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
                check(vm, a, $f)
            }
        )*
    };
}

ctypes! {
    ctype_alnum => |c| c.is_ascii_alphanumeric(),
    ctype_alpha => |c| c.is_ascii_alphabetic(),
    ctype_cntrl => |c| c.is_ascii_control(),
    ctype_digit => |c| c.is_ascii_digit(),
    ctype_graph => |c| c.is_ascii_graphic(),
    ctype_lower => |c| c.is_ascii_lowercase(),
    ctype_print => |c| c.is_ascii_graphic() || c == b' ',
    ctype_punct => |c| c.is_ascii_punctuation(),
    ctype_space => |c| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c),
    ctype_upper => |c| c.is_ascii_uppercase(),
    ctype_xdigit => |c| c.is_ascii_hexdigit(),
}
