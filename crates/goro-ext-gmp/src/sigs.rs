//! Signatures of the gmp functions (observed on PHP 8.5 through
//! Reflection), for the engine's parameter parsing.

/// A parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Param {
    pub name: &'static str,
    /// The declared type as PHP prints it.
    pub ty: &'static str,
    /// The default value as PHP source (`None`: required).
    pub default: Option<&'static str>,
}

/// A function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FnSig {
    pub name: &'static str,
    pub params: &'static [Param],
    pub ret: &'static str,
}

const fn p(name: &'static str, ty: &'static str) -> Param {
    Param {
        name,
        ty,
        default: None,
    }
}

const fn d(name: &'static str, ty: &'static str, default: &'static str) -> Param {
    Param {
        name,
        ty,
        default: Some(default),
    }
}

const NUM: &str = "GMP|string|int";

const fn f(name: &'static str, params: &'static [Param], ret: &'static str) -> FnSig {
    FnSig { name, params, ret }
}

const ONE: &[Param] = &[p("num", NUM)];
const TWO: &[Param] = &[p("num1", NUM), p("num2", NUM)];
const DIV: &[Param] = &[
    p("num1", NUM),
    p("num2", NUM),
    d("rounding_mode", "int", "GMP_ROUND_ZERO"),
];

/// Every gmp function, in PHP's registration order, and the `GMP` methods
/// (named `GMP::...`).
pub const SIGNATURES: &[FnSig] = &[
    f(
        "gmp_init",
        &[p("num", "string|int"), d("base", "int", "0")],
        "GMP",
    ),
    f(
        "gmp_import",
        &[
            p("data", "string"),
            d("word_size", "int", "1"),
            d("flags", "int", "GMP_MSW_FIRST | GMP_NATIVE_ENDIAN"),
        ],
        "GMP",
    ),
    f(
        "gmp_export",
        &[
            p("num", NUM),
            d("word_size", "int", "1"),
            d("flags", "int", "GMP_MSW_FIRST | GMP_NATIVE_ENDIAN"),
        ],
        "string",
    ),
    f("gmp_intval", ONE, "int"),
    f(
        "gmp_strval",
        &[p("num", NUM), d("base", "int", "10")],
        "string",
    ),
    f("gmp_add", TWO, "GMP"),
    f("gmp_sub", TWO, "GMP"),
    f("gmp_mul", TWO, "GMP"),
    f("gmp_div_qr", DIV, "array"),
    f("gmp_div_q", DIV, "GMP"),
    f("gmp_div_r", DIV, "GMP"),
    f("gmp_div", DIV, "GMP"),
    f("gmp_mod", TWO, "GMP"),
    f("gmp_divexact", TWO, "GMP"),
    f("gmp_neg", ONE, "GMP"),
    f("gmp_abs", ONE, "GMP"),
    f("gmp_fact", ONE, "GMP"),
    f("gmp_sqrt", ONE, "GMP"),
    f("gmp_sqrtrem", ONE, "array"),
    f("gmp_root", &[p("num", NUM), p("nth", "int")], "GMP"),
    f("gmp_rootrem", &[p("num", NUM), p("nth", "int")], "array"),
    f("gmp_pow", &[p("num", NUM), p("exponent", "int")], "GMP"),
    f(
        "gmp_powm",
        &[p("num", NUM), p("exponent", NUM), p("modulus", NUM)],
        "GMP",
    ),
    f("gmp_perfect_square", ONE, "bool"),
    f("gmp_perfect_power", ONE, "bool"),
    f(
        "gmp_prob_prime",
        &[p("num", NUM), d("repetitions", "int", "10")],
        "int",
    ),
    f("gmp_gcd", TWO, "GMP"),
    f("gmp_gcdext", TWO, "array"),
    f("gmp_lcm", TWO, "GMP"),
    f("gmp_invert", TWO, "GMP|false"),
    f("gmp_jacobi", TWO, "int"),
    f("gmp_legendre", TWO, "int"),
    f("gmp_kronecker", TWO, "int"),
    f("gmp_cmp", TWO, "int"),
    f("gmp_sign", ONE, "int"),
    f("gmp_random_seed", &[p("seed", NUM)], "void"),
    f("gmp_random_bits", &[p("bits", "int")], "GMP"),
    f("gmp_random_range", &[p("min", NUM), p("max", NUM)], "GMP"),
    f("gmp_and", TWO, "GMP"),
    f("gmp_or", TWO, "GMP"),
    f("gmp_com", ONE, "GMP"),
    f("gmp_xor", TWO, "GMP"),
    f(
        "gmp_setbit",
        &[
            p("num", "GMP"),
            p("index", "int"),
            d("value", "bool", "true"),
        ],
        "void",
    ),
    f("gmp_clrbit", &[p("num", "GMP"), p("index", "int")], "void"),
    f("gmp_testbit", &[p("num", NUM), p("index", "int")], "bool"),
    f("gmp_scan0", &[p("num1", NUM), p("start", "int")], "int"),
    f("gmp_scan1", &[p("num1", NUM), p("start", "int")], "int"),
    f("gmp_popcount", ONE, "int"),
    f("gmp_hamdist", TWO, "int"),
    f("gmp_nextprime", ONE, "GMP"),
    f("gmp_binomial", &[p("n", NUM), p("k", "int")], "GMP"),
    f(
        "GMP::__construct",
        &[d("num", "string|int", "0"), d("base", "int", "0")],
        "",
    ),
    f("GMP::__serialize", &[], "array"),
    f("GMP::__unserialize", &[p("data", "array")], "void"),
];
