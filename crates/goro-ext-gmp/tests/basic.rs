//! Hand-written checks of required behaviour, each citing the php-src
//! test it comes from.

use goro_ext_gmp::*;
use goro_runtime::Diagnostics;

fn s(v: &str) -> Num<'_> {
    Num::Str(v.as_bytes())
}

#[test]
fn random_seed_streams() {
    // test ext/gmp/tests/gmp_random_seed.phpt
    let mut d = Diagnostics::default();
    let mut env = Env::new(&mut d, false);
    let expect_zero = [
        "107",
        "576055025228722307492589900056",
        "5075491613651149525976453192895895253653438900772590630831858908690082668789318258254821002217677675804439098856210618572534955562143303188483908287009522532300439665975877709754914215718998849272363858786685187951932478210775857465448084868199807983919191214972626993925394176279001074206804955195464",
        "4098",
        "866",
        "-4602",
    ];
    let expect_100 = [
        "800",
        "136797365759249926716355081555",
        "983682312243221532860194306859606025979259367996304596374614332718375645173854152266611727577102182844028492473112400528817154210713755887896949560718745264129216953815968005630126359941634684721501777057142617647654380585317016323758806063124938232519551123440573348326061244006512869165793958775168",
        "1029",
        "7093",
        "-9074",
    ];
    let expect_big = [
        "762",
        "822340340897453415684831711085",
        "7240560133683902061389868703829443708354917824328579773726122219756981024103097560162756171513655189995985599958252688592185764428631571614485572869738344560301294144844739876478557439580966605216861285841689262517286639329902832431755450003123084728943981078635297917573398492558065003906539489023830",
        "9636",
        "-9848",
        "-9648",
    ];
    let big = Gmp(Int::ONE.mul_2k(512));
    let nbig = Gmp(big.0.neg());
    for (seed, expect) in [
        (Num::Int(0), &expect_zero),
        (Num::Int(-100), &expect_100),
        (Num::Int(100), &expect_100),
        (Num::Gmp(&nbig), &expect_big),
        (Num::Gmp(&big), &expect_big),
    ] {
        gmp_random_seed(&mut env, seed).unwrap();
        let got = [
            gmp_random_bits(10).unwrap().to_string(),
            gmp_random_bits(100).unwrap().to_string(),
            gmp_random_bits(1000).unwrap().to_string(),
            gmp_random_range(&mut env, Num::Int(0), Num::Int(10000))
                .unwrap()
                .to_string(),
            gmp_random_range(&mut env, Num::Int(-10000), Num::Int(10000))
                .unwrap()
                .to_string(),
            gmp_random_range(&mut env, Num::Int(-10000), Num::Int(0))
                .unwrap()
                .to_string(),
        ];
        assert_eq!(&got, expect, "{seed:?}");
    }
    let e = gmp_random_seed(&mut env, s("not a number")).unwrap_err();
    assert_eq!(
        e.message,
        "gmp_random_seed(): Argument #1 ($seed) is not an integer string"
    );
}

#[test]
fn messages() {
    let mut d = Diagnostics::default();
    let mut env = Env::new(&mut d, false);
    // test ext/gmp/tests/gmp_abs.phpt
    let e = gmp_abs(&mut env, Num::Other("array")).unwrap_err();
    assert_eq!(
        e.message,
        "gmp_abs(): Argument #1 ($num) must be of type GMP|string|int, array given"
    );
    // test ext/gmp/tests/bug32773.phpt
    let e = gmp_div(&mut env, Num::Int(10), Num::Int(0), 0).unwrap_err();
    assert_eq!(e.message, "gmp_div(): Argument #2 ($num2) Division by zero");
    assert_eq!(e.class_name(), "DivisionByZeroError");
    // test ext/gmp/tests/gmp_strict_types.phpt
    let mut env = Env::new(&mut d, true);
    let e = gmp_abs(&mut env, Num::Bool(false)).unwrap_err();
    assert_eq!(
        e.message,
        "gmp_abs(): Argument #1 ($num) must be of type GMP|string|int, false given"
    );
}

#[test]
fn constructor() {
    // test ext/gmp/tests/construct.phpt
    let mut d = Diagnostics::default();
    let mut env = Env::new(&mut d, false);
    let c = |env: &mut Env, n: Option<Num>, b: i64| gmp_construct(env, n, b).map(|g| g.to_string());
    assert_eq!(c(&mut env, None, 0).unwrap(), "0");
    assert_eq!(c(&mut env, Some(Num::Int(123)), 0).unwrap(), "123");
    assert_eq!(c(&mut env, Some(s("0xAA")), 0).unwrap(), "170");
    assert_eq!(c(&mut env, Some(s("12")), 4).unwrap(), "6");
    assert_eq!(
        c(&mut env, Some(s("12")), 999).unwrap_err().message,
        "GMP::__construct(): Argument #2 ($base) must be 0 or between 2 and 62"
    );
    for v in ["", "hello"] {
        assert_eq!(
            c(&mut env, Some(s(v)), 0).unwrap_err().message,
            "GMP::__construct(): Argument #1 ($num) is not an integer string"
        );
    }
}

#[test]
fn prob_prime_phpt() {
    // test ext/gmp/tests/gmp_prob_prime.phpt
    let mut d = Diagnostics::default();
    let mut env = Env::new(&mut d, false);
    let mut got = Vec::new();
    for v in ["10", "7", "17", "-31", "172368715471481723"] {
        got.push(gmp_prob_prime(&mut env, s(v), 10).unwrap());
    }
    assert_eq!(got, [0, 2, 2, 2, 0]);
    let n = Gmp(parse_integer(b"23476812735411", 10).unwrap());
    let mut got = Vec::new();
    for i in -1i64..12 {
        got.push(gmp_prob_prime(&mut env, Num::Int(773 * i - i * 7 - 1), i).unwrap());
        let m = gmp_add(&mut env, Num::Gmp(&n), Num::Int(i - 1)).unwrap();
        got.push(gmp_prob_prime(&mut env, Num::Gmp(&m), i).unwrap());
    }
    let mut expect = [0; 26];
    expect[6] = 2;
    expect[8] = 2;
    assert_eq!(got, expect);
}

#[test]
fn bit_and_export_errors() {
    let mut d = Diagnostics::default();
    let mut env = Env::new(&mut d, false);
    // test ext/gmp/tests/gmp_setbit.phpt
    let mut n = Gmp::from_i64(-1);
    gmp_setbit(&mut n, 10, true).unwrap();
    assert_eq!(n.to_string(), "-1");
    let e = gmp_setbit(&mut Gmp::from_i64(5), -20, false).unwrap_err();
    assert_eq!(
        e.message,
        "gmp_setbit(): Argument #2 ($index) must be between 0 and 2147483647 * 64"
    );
    assert_eq!(
        gmp_object_type_error("gmp_setbit", "string").message,
        "gmp_setbit(): Argument #1 ($num) must be of type GMP, string given"
    );
    // test ext/gmp/tests/gh16411.phpt
    let e = gmp_export(&mut env, s("-9223372036854775808"), i64::MAX, i64::MIN).unwrap_err();
    assert_eq!(
        e.message,
        "gmp_export(): Argument #2 ($word_size) is too large for argument #1 ($num)"
    );
    // test ext/gmp/tests/gmp_export.phpt
    let flags = GMP_MSW_FIRST | GMP_LSW_FIRST;
    let e = gmp_export(&mut env, Num::Int(123), 1, flags).unwrap_err();
    assert_eq!(
        e.message,
        "gmp_export(): Argument #3 ($flags) cannot use multiple word order options"
    );
    assert_eq!(gmp_export(&mut env, Num::Int(0xff), 1, 17).unwrap(), [0xff]);
    // test ext/gmp/tests/gmp_remroot.phpt
    let e = gmp_rootrem(&mut env, Num::Int(-100), 4).unwrap_err();
    assert_eq!(
        e.message,
        "gmp_rootrem(): Argument #2 ($nth) must be odd if argument #1 ($a) is negative"
    );
    let (r, m) = gmp_rootrem(&mut env, Num::Int(-100), 3).unwrap();
    assert_eq!((r.to_string(), m.to_string()), ("-4".into(), "-36".into()));
}
