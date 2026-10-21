//! Command line option parsing, ported from main/getopt.c (php_getopt).

/// One entry of the option table (opt_struct).
pub struct Opt {
    pub opt_char: i32,
    /// 0 = no argument, 1 = required argument, 2 = optional (`--name=value` only)
    pub need_param: u8,
    pub opt_name: &'static str,
}

const fn o(opt_char: i32, need_param: u8, opt_name: &'static str) -> Opt {
    Opt {
        opt_char,
        need_param,
        opt_name,
    }
}

/// sapi/cli/php_cli.c OPTIONS[]
pub const OPTIONS: &[Opt] = &[
    o('a' as i32, 0, "interactive"),
    o('B' as i32, 1, "process-begin"),
    o('C' as i32, 0, "no-chdir"),
    o('c' as i32, 1, "php-ini"),
    o('d' as i32, 1, "define"),
    o('E' as i32, 1, "process-end"),
    o('e' as i32, 0, "profile-info"),
    o('F' as i32, 1, "process-file"),
    o('f' as i32, 1, "file"),
    o('h' as i32, 0, "help"),
    o('i' as i32, 0, "info"),
    o('l' as i32, 0, "syntax-check"),
    o('m' as i32, 0, "modules"),
    o('n' as i32, 0, "no-php-ini"),
    o('q' as i32, 0, "no-header"),
    o('R' as i32, 1, "process-code"),
    o('H' as i32, 0, "hide-args"),
    o('r' as i32, 1, "run"),
    o('s' as i32, 0, "syntax-highlight"),
    o('s' as i32, 0, "syntax-highlighting"),
    o('S' as i32, 1, "server"),
    o('t' as i32, 1, "docroot"),
    o('w' as i32, 0, "strip"),
    o('?' as i32, 0, "usage"),
    o('v' as i32, 0, "version"),
    o(10, 1, "rf"),
    o(10, 1, "rfunction"),
    o(11, 1, "rc"),
    o(11, 1, "rclass"),
    o(12, 1, "re"),
    o(12, 1, "rextension"),
    o(13, 1, "rz"),
    o(13, 1, "rzendextension"),
    o(14, 1, "ri"),
    o(14, 1, "rextinfo"),
    o(15, 2, "ini"),
    o(16, 1, "repeat"),
];

pub const GETOPT_INVALID_ARG: i32 = -2;

const OPTERRCOLON: i32 = 1;
const OPTERRNF: i32 = 2;
const OPTERRARG: i32 = 3;

/// Parser state (the static locals of php_getopt) plus optind.
pub struct GetOpt<'a> {
    argv: &'a [Vec<u8>],
    pub optind: usize,
    optchr: usize,
    dash: bool,
    show_err: bool,
}

/// Byte at `i` of `s`, or NUL past the end (C string semantics).
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

impl<'a> GetOpt<'a> {
    pub fn new(argv: &'a [Vec<u8>], show_err: bool) -> Self {
        GetOpt {
            argv,
            optind: 1,
            optchr: 0,
            dash: false,
            show_err,
        }
    }

    fn error(&self, optint: usize, optchr: usize, err: i32) -> i32 {
        if self.show_err {
            let mut msg = format!("Error in argument {}, char {}: ", optint, optchr + 1);
            match err {
                OPTERRCOLON => msg.push_str(": in flags\n"),
                OPTERRNF => msg.push_str(&format!(
                    "option not found {}\n",
                    at(&self.argv[optint], optchr) as char
                )),
                OPTERRARG => msg.push_str(&format!(
                    "no argument for option {}\n",
                    at(&self.argv[optint], optchr) as char
                )),
                _ => msg.push_str("unknown\n"),
            }
            use std::io::Write;
            let _ = std::io::stderr().write_all(msg.as_bytes());
        }
        GETOPT_INVALID_ARG
    }

    /// Returns the option character, -1 at the end of the options, or
    /// GETOPT_INVALID_ARG. `optarg` receives the option argument.
    pub fn next(&mut self, optarg: &mut Option<Vec<u8>>) -> i32 {
        let argc = self.argv.len();
        let mut optidx: isize = -1;
        if self.optind >= argc {
            return -1;
        }
        let cur = &self.argv[self.optind];
        if !self.dash {
            if at(cur, 0) != b'-' {
                return -1;
            } else if at(cur, 1) == 0 {
                // "-" alone: stdin, let the program process it
                return -1;
            }
        }
        let mut arg_start: usize;
        if at(cur, 0) == b'-' && at(cur, 1) == b'-' {
            // '--' indicates end of args if not followed by a known long option name
            if at(cur, 2) == 0 {
                self.optind += 1;
                return -1;
            }
            arg_start = 2;
            let mut arg_end = cur.len() - 1;
            // Check for <arg>=<val>
            let search = &cur[arg_start..arg_start + (arg_end - arg_start)];
            if let Some(p) = search.iter().position(|&c| c == b'=') {
                arg_end = p;
                arg_start += 1;
            } else {
                arg_end -= 1;
            }
            loop {
                optidx += 1;
                if optidx as usize >= OPTIONS.len() {
                    self.optind += 1;
                    return self.error(self.optind - 1, self.optchr, OPTERRARG);
                }
                let name = OPTIONS[optidx as usize].opt_name.as_bytes();
                // strncmp(&argv[2], name, arg_end) && arg_end == strlen(name)
                if arg_end == name.len() && cur.len() >= 2 + arg_end && &cur[2..2 + arg_end] == name
                {
                    break;
                }
            }
            self.optchr = 0;
            self.dash = false;
            arg_start += OPTIONS[optidx as usize].opt_name.len();
        } else {
            if !self.dash {
                self.dash = true;
                self.optchr = 1;
            }
            // Check if the guy tries to do a -: kind of flag
            if at(cur, self.optchr) == b':' {
                self.dash = false;
                self.optind += 1;
                return self.error(self.optind - 1, self.optchr, OPTERRCOLON);
            }
            arg_start = 1 + self.optchr;
        }
        if optidx < 0 {
            loop {
                optidx += 1;
                if optidx as usize >= OPTIONS.len() {
                    let errind = self.optind;
                    let errchr = self.optchr;
                    if at(cur, self.optchr + 1) == 0 {
                        self.dash = false;
                        self.optind += 1;
                    } else {
                        self.optchr += 1;
                    }
                    return self.error(errind, errchr, OPTERRNF);
                } else if at(cur, self.optchr) as i32 == OPTIONS[optidx as usize].opt_char {
                    break;
                }
            }
        }
        let opt = &OPTIONS[optidx as usize];
        if opt.need_param != 0 {
            // -<arg> <val>, -<arg>=<val> or -<arg><val>
            self.dash = false;
            if at(cur, arg_start) == 0 {
                self.optind += 1;
                if self.optind == argc {
                    if opt.need_param == 1 {
                        return self.error(self.optind - 1, self.optchr, OPTERRARG);
                    }
                } else if opt.need_param == 1 {
                    *optarg = Some(self.argv[self.optind].clone());
                    self.optind += 1;
                }
            } else if at(cur, arg_start) == b'=' {
                *optarg = Some(cur[arg_start + 1..].to_vec());
                self.optind += 1;
            } else {
                *optarg = Some(cur[arg_start..].to_vec());
                self.optind += 1;
            }
            opt.opt_char
        } else {
            // multiple options specified as one (exclude long opts)
            if arg_start >= 2 && !(at(cur, 0) == b'-' && at(cur, 1) == b'-') {
                if at(cur, self.optchr + 1) == 0 {
                    self.dash = false;
                    self.optind += 1;
                } else {
                    self.optchr += 1;
                }
            } else {
                self.optind += 1;
            }
            opt.opt_char
        }
    }
}
