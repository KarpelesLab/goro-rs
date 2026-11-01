//! The phpdbg SAPI: `goro --phpdbg` (or goro run as `phpdbg`), the
//! interactive debugger. Commands come from stdin, one per line (`#`
//! separates several on a line); the script runs under the debugger with
//! breakpoints, stepping and watchpoints (PHP manual: phpdbg, Command line
//! options, Commands; behaviour as the tests in sapi/phpdbg/tests expect:
//! the "prompt> " before each line read, the bracketed messages, the
//! three-line source listing at each stop).
//!
//! The engine compiles code with statement markers (`Op::Ticks`, see
//! [`crate::compiler::DEBUG_MARKERS`]) and calls [`on_marker`] at each
//! marker, [`on_exception`] each time an exception looks for a handler in a
//! frame; both act only while a script runs under the debugger. Breakpoints
//! and stepping work per statement, not per Zend opline: an opcode
//! breakpoint (`b ZEND_ECHO`) stops at the statements containing an
//! instruction of that kind. Opline listings (`print`) and the oplog's
//! per-opline counts need Zend's opcodes and are not available.
//!
//! Each `run` starts a fresh engine; between runs an idle engine evaluates
//! `ev` code and registered function calls, and holds the compiled script.

mod commands;
mod funcs;
mod watch;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, Write};
use std::rc::Rc;

use goro_runtime::{ObjRef, PhpStr, Value, WeakObj};

use crate::bytecode::{FuncCode, Op, fflags};
use crate::vm::{R, Unwind, Vm};

pub use funcs::PHPDBG_MODULE;

/// A breakpoint.
pub(crate) struct Bp {
    pub id: u32,
    pub kind: BpKind,
    pub hits: u32,
}

#[derive(Clone, PartialEq)]
pub(crate) enum BpKind {
    /// `b [file:]line`
    Line { file: Vec<u8>, line: u32 },
    /// `b func` (lower-case name, as typed)
    Func { lname: Vec<u8>, shown: String },
    /// `b class::method`
    Method {
        class: Vec<u8>,
        method: Vec<u8>,
        shown: String,
    },
    /// `b ZEND_ECHO`
    Opcode { name: String },
    /// phpdbg_break_next(): the next statement, once.
    Next { addr: usize, done: bool },
}

impl BpKind {
    fn desc(&self) -> String {
        match self {
            BpKind::Line { file, line } => format!("{}:{}", String::from_utf8_lossy(file), line),
            BpKind::Func { shown, .. } | BpKind::Method { shown, .. } => shown.clone(),
            BpKind::Opcode { name } => name.clone(),
            BpKind::Next { addr, .. } => format!("{:#x}", addr),
        }
    }

    fn same(&self, o: &BpKind) -> bool {
        match (self, o) {
            (BpKind::Line { file: a, line: l }, BpKind::Line { file: b, line: m }) => {
                a == b && l == m
            }
            (BpKind::Func { lname: a, .. }, BpKind::Func { lname: b, .. }) => a == b,
            (
                BpKind::Method {
                    class: a,
                    method: m,
                    ..
                },
                BpKind::Method {
                    class: b,
                    method: n,
                    ..
                },
            ) => a == b && m == n,
            (BpKind::Opcode { name: a }, BpKind::Opcode { name: b }) => a == b,
            _ => false,
        }
    }
}

/// Where to stop next when stepping.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Step {
    /// `step`: at the next statement on another line, in any frame.
    Into {
        depth: usize,
        code: usize,
        line: u32,
    },
    /// `next`: the same, without stopping in called functions.
    Over {
        depth: usize,
        code: usize,
        line: u32,
    },
    /// `finish`: once the current function has returned.
    Finish { depth: usize },
    /// `leave`: at the current function's return.
    Leave { depth: usize },
}

/// How a run ended, or what a command asked of the running script.
pub(crate) enum RunEnd {
    Done,
    /// `run` while running: start again with these arguments and stdin.
    Restart(Option<Vec<Vec<u8>>>, Option<Vec<u8>>),
    /// `clean`, `exec`: stop the script.
    Stop,
}

/// What the command loop returns.
pub(crate) enum Flow {
    /// Resume the script (or stay idle).
    Resume,
    /// Start the script (arguments, stdin file).
    Run(Option<Vec<Vec<u8>>>, Option<Vec<u8>>),
    /// Abandon the running script.
    Abort(RunEnd),
}

/// The execution context (`exec`, the script on the command line, `stdin`).
#[derive(Clone)]
pub(crate) struct Script {
    pub path: Vec<u8>,
    pub src: Rc<Vec<u8>>,
}

/// Where the script is stopped.
#[derive(Clone)]
pub(crate) struct Pos {
    pub file: Vec<u8>,
    pub line: u32,
    pub ret_marker: bool,
}

#[derive(Default)]
pub(crate) struct Dbg {
    pub prompt: String,
    pub ini: Vec<(Vec<u8>, Vec<u8>)>,
    pub script: Option<Script>,
    pub unit: Option<crate::compiler::Unit>,
    /// The arguments given on the command line (`$argv[1..]`).
    pub args: Vec<Vec<u8>>,
    pub bps: Vec<Bp>,
    pub next_bp: u32,
    pub step: Option<Step>,
    pub watches: Vec<watch::Watch>,
    pub next_watch: u32,
    pub registered: Vec<Vec<u8>>,
    pub input: Option<Box<dyn BufRead>>,
    pub queue: VecDeque<String>,
    pub last_line: Option<String>,
    pub eof: bool,
    pub running: bool,
    pub in_hook: bool,
    /// "[Script ended normally]" waits for the next prompt (a `quit` on
    /// the same line drops it: test sapi/phpdbg/tests/bug73794.phpt).
    pub pending_end: bool,
    /// The output is at the start of a line.
    pub line_start: bool,
    pub seen_written: u64,
    /// The last marker run: (depth, code, line).
    pub last_marker: (usize, usize, u32),
    /// Exceptions seen by the dispatcher: where each was thrown (depth,
    /// line).
    pub seen_ex: Vec<(WeakObj, usize, u32)>,
    /// The last run ended with an uncaught exception.
    pub last_uncaught: bool,
    pub abort: Option<RunEnd>,
    pub cur: Option<Pos>,
    pub oplog: bool,
    pub sources: HashMap<Vec<u8>, Rc<Vec<u8>>>,
}

thread_local! {
    static DBG: RefCell<Dbg> = RefCell::new(Dbg::default());
}

pub(crate) fn with<T>(f: impl FnOnce(&mut Dbg) -> T) -> T {
    DBG.with(|d| f(&mut d.borrow_mut()))
}

// ------------------------------------------------------------ output

/// Flushes the script's output, noting whether it ended a line.
pub(crate) fn sync_out(vm: &mut Vm) {
    let w = vm.out.written;
    let last = vm.out.stdout.last().copied();
    with(|d| {
        if w != d.seen_written {
            d.line_start = last == Some(b'\n');
            d.seen_written = w;
        }
    });
    vm.out.flush();
}

/// Writes debugger text as is.
pub(crate) fn write_raw(s: &[u8]) {
    if s.is_empty() {
        return;
    }
    let mut so = std::io::stdout().lock();
    let _ = so.write_all(s);
    let _ = so.flush();
    with(|d| d.line_start = s.last() == Some(&b'\n'));
}

/// Writes a message on a line of its own.
pub(crate) fn msg(vm: &mut Vm, s: &str) {
    sync_out(vm);
    if !with(|d| d.line_start) {
        write_raw(b"\n");
    }
    let mut b = s.as_bytes().to_vec();
    b.push(b'\n');
    write_raw(&b);
}

fn flush_pending_end(vm: &mut Vm) {
    if with(|d| std::mem::take(&mut d.pending_end)) {
        msg(vm, "[Script ended normally]");
    }
}

/// The source of a file, for listings.
pub(crate) fn source_of(file: &[u8]) -> Rc<Vec<u8>> {
    if let Some(s) = with(|d| d.sources.get(file).cloned()) {
        return s;
    }
    use std::os::unix::ffi::OsStrExt;
    let s = Rc::new(std::fs::read(std::ffi::OsStr::from_bytes(file)).unwrap_or_default());
    with(|d| d.sources.insert(file.to_vec(), s.clone()));
    s
}

/// The listing shown at a stop: `n` lines from `line`, the current one
/// marked with `>`.
pub(crate) fn listing(vm: &mut Vm, file: &[u8], line: u32, n: u32, mark: bool) {
    sync_out(vm);
    if !with(|d| d.line_start) {
        write_raw(b"\n");
    }
    let src = source_of(file);
    let lines: Vec<&[u8]> = src.split(|&b| b == b'\n').collect();
    let mut out = Vec::new();
    for l in line..line.saturating_add(n) {
        if l == 0 || l as usize > lines.len() {
            break;
        }
        let text = lines[l as usize - 1];
        let text = text.strip_suffix(b"\r").unwrap_or(text);
        out.push(if mark && l == line { b'>' } else { b' ' });
        out.extend_from_slice(format!("{:05}: ", l).as_bytes());
        out.extend_from_slice(text);
        out.push(b'\n');
    }
    write_raw(&out);
}

// ------------------------------------------------------------ input

fn read_input_line() -> Option<String> {
    with(|d| {
        let input = d.input.as_mut()?;
        let mut buf = Vec::new();
        match input.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                }
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                Some(String::from_utf8_lossy(&buf).into_owned())
            }
        }
    })
}

/// A raw line of input (the answer to a question, `stdin` code).
pub(crate) fn read_raw_line() -> Option<String> {
    if let Some(c) = with(|d| d.queue.pop_front()) {
        return Some(c);
    }
    read_input_line()
}

/// Asks a yes/no question.
pub(crate) fn confirm(vm: &mut Vm, question: &str) -> bool {
    sync_out(vm);
    write_raw(question.as_bytes());
    with(|d| d.line_start = true);
    match read_raw_line() {
        Some(a) => a.trim_start().starts_with(['y', 'Y']),
        None => false,
    }
}

/// Splits a line into commands at `#`.
fn queue_line(line: &str) -> String {
    let mut parts = line.split('#').map(|p| p.trim().to_string());
    let first = parts.next().unwrap_or_default();
    let rest: Vec<String> = parts.filter(|p| !p.is_empty()).collect();
    with(|d| {
        for (i, p) in rest.into_iter().enumerate() {
            d.queue.insert(i, p);
        }
    });
    first
}

/// The next command: queued ones first, then a line of input after the
/// prompt. An empty line repeats the last one; the first end of input
/// does too, the second quits (tests sapi/phpdbg/tests/gh15901.phpt,
/// gh16174.phpt). `None`: quit.
fn next_command(vm: &mut Vm) -> Option<String> {
    if let Some(c) = with(|d| d.queue.pop_front()) {
        return Some(c);
    }
    flush_pending_end(vm);
    sync_out(vm);
    let p = with(|d| d.prompt.clone());
    write_raw(p.as_bytes());
    with(|d| d.line_start = true);
    let line = match read_input_line() {
        Some(l) if l.trim().is_empty() => with(|d| d.last_line.clone()).unwrap_or_default(),
        Some(l) => {
            with(|d| d.last_line = Some(l.clone()));
            l
        }
        None => {
            if with(|d| std::mem::replace(&mut d.eof, true)) {
                return None;
            }
            with(|d| d.last_line.clone()).unwrap_or_default()
        }
    };
    Some(queue_line(&line))
}

/// `quit`: ends the process.
pub(crate) fn quit(vm: &mut Vm) -> ! {
    if with(|d| d.running) {
        watch::remove_all(vm);
    }
    sync_out(vm);
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}

/// Reads and runs commands until one resumes or starts the script.
pub(crate) fn command_loop(vm: &mut Vm) -> Flow {
    loop {
        let Some(line) = next_command(vm) else {
            quit(vm);
        };
        if let Some(f) = commands::run_command(vm, &line) {
            return f;
        }
    }
}

// ------------------------------------------------------------ engines

fn make_vm(ini: &[(Vec<u8>, Vec<u8>)]) -> Vm {
    let mut vm = crate::startup(ini, &[&PHPDBG_MODULE]);
    vm.constants.insert(
        PhpStr::intern(b"PHP_SAPI"),
        Value::Str(PhpStr::from("phpdbg")),
    );
    // The debugger flushes the output itself, to know where lines end.
    vm.out.implicit_flush = false;
    vm
}

/// Sets the execution context: reads and compiles the file.
pub(crate) fn set_context(vm: &mut Vm, path: &[u8], announce: bool) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let p = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
    let src = match std::fs::read(p) {
        Ok(s) if p.is_file() => s,
        _ => {
            msg(
                vm,
                &format!(
                    "[Cannot stat {}, ensure the file exists]",
                    String::from_utf8_lossy(path)
                ),
            );
            return false;
        }
    };
    let abs = std::fs::canonicalize(p)
        .map(|a| a.as_os_str().as_bytes().to_vec())
        .unwrap_or_else(|_| path.to_vec());
    if with(|d| d.script.as_ref().is_some_and(|s| s.path == abs)) {
        msg(vm, "[Execution context not changed]");
        return false;
    }
    if announce {
        msg(
            vm,
            &format!("[Set execution context: {}]", String::from_utf8_lossy(&abs)),
        );
    }
    let src = Rc::new(src);
    with(|d| {
        d.sources.insert(abs.clone(), src.clone());
        d.script = Some(Script {
            path: abs.clone(),
            src: src.clone(),
        });
    });
    compile_context(vm, &abs, &src, &String::from_utf8_lossy(&abs));
    true
}

/// Compiles the execution context in the idle engine.
pub(crate) fn compile_context(vm: &mut Vm, path: &[u8], src: &[u8], shown: &str) {
    let code = crate::strip_shebang(src.to_vec());
    let unit = vm.precompile(&code, path);
    let ok = unit.is_some();
    with(|d| d.unit = unit);
    if ok {
        msg(vm, &format!("[Successful compilation of {}]", shown));
    } else {
        sync_out(vm);
    }
}

/// Runs the execution context once.
fn do_run(args: Option<Vec<Vec<u8>>>, stdin: Option<Vec<u8>>) -> RunEnd {
    let Some(script) = with(|d| d.script.clone()) else {
        return RunEnd::Done;
    };
    let ini = with(|d| d.ini.clone());
    let mut vm = make_vm(&ini);
    let args = args.unwrap_or_else(|| with(|d| d.args.clone()));
    let mut argv = vec![script.path.clone()];
    argv.extend(args);
    vm.set_script(&script.path, &argv);
    let key = PhpStr::new(&script.path);
    vm.included.push(key.clone());
    vm.included_set.insert(key);
    crate::builtins::output::start_ini_handler(&mut vm);
    vm.arm_stack_limit();
    vm.arm_memory_limit();
    with(|d| {
        d.running = true;
        d.step = None;
        d.seen_ex.clear();
        d.last_marker = (0, 0, 0);
        d.last_uncaught = false;
        d.abort = None;
        d.seen_written = 0;
        for b in &mut d.bps {
            b.hits = 0;
        }
    });
    // `run < file`: the script's stdin (commands keep coming from the
    // original one).
    let saved_stdin = stdin.and_then(|f| {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::AsRawFd;
        let file = std::fs::File::open(std::ffi::OsStr::from_bytes(&f)).ok()?;
        // SAFETY: plain descriptor duplication.
        unsafe {
            let saved = libc::dup(0);
            libc::dup2(file.as_raw_fd(), 0);
            Some(saved)
        }
    });
    let src = if script.path == b"Standard input code" {
        script.src.to_vec()
    } else {
        crate::strip_shebang(script.src.to_vec())
    };
    let r = vm.run_main_code(&src, &script.path);
    if let Some(fd) = saved_stdin {
        // SAFETY: restores the descriptor saved above.
        unsafe {
            libc::dup2(fd, 0);
            libc::close(fd);
        }
    }
    if let Some(end) = with(|d| d.abort.take()) {
        abandon(&mut vm);
        return end;
    }
    match r {
        Err(Unwind::Throw(e)) => uncaught_end(&mut vm, e),
        Err(Unwind::Fatal) | Err(Unwind::Exit) | Ok(()) => {
            vm.ex.frames.clear();
            vm.ex.pending.clear();
            vm.ex.native_calls.clear();
        }
    }
    watch::remove_all(&mut vm);
    // Seen now: shutdown flushes the output itself.
    sync_out(&mut vm);
    vm.shutdown();
    if let Some(end) = with(|d| d.abort.take()) {
        abandon(&mut vm);
        return end;
    }
    sync_out(&mut vm);
    with(|d| {
        d.running = false;
        d.pending_end = true;
        d.watches.clear();
    });
    RunEnd::Done
}

/// Drops a script stopped midway: its objects get no destructor call.
fn abandon(vm: &mut Vm) {
    sync_out(vm);
    for o in goro_runtime::object::live_objects() {
        o.mark_destructor_called();
    }
    vm.ex.frames.clear();
    vm.ex.pending.clear();
    vm.ex.native_calls.clear();
    with(|d| {
        d.running = false;
        d.watches.clear();
    });
}

/// An exception nobody caught: the script's exception handler, or the
/// exception shown (tests sapi/phpdbg/tests/exceptions_001.phpt,
/// set_exception_handler.phpt).
fn uncaught_end(vm: &mut Vm, e: ObjRef) {
    vm.ex.frames.clear();
    vm.ex.pending.clear();
    vm.ex.native_calls.clear();
    with(|d| d.last_uncaught = true);
    if vm
        .err
        .exception_handlers
        .last()
        .is_some_and(|h| !h.is_null())
    {
        vm.report_uncaught(e);
        return;
    }
    let (cls, file, line) = exception_place(vm, &e);
    let text = match vm.exception_to_string(&e) {
        Ok(s) => s,
        Err(_) => cls.clone(),
    };
    msg(
        vm,
        &format!(
            "[Uncaught {} in {} on line {}]",
            cls,
            String::from_utf8_lossy(&file),
            line
        ),
    );
    write_raw(format!("{}\n", text).as_bytes());
}

/// An exception's class, file and line.
pub(crate) fn exception_place(vm: &mut Vm, e: &ObjRef) -> (String, Vec<u8>, u32) {
    let cls = vm.class_of(e).name.to_string_lossy();
    let file = match vm.get_prop_raw(e, b"file") {
        Value::Str(s) => s.to_vec(),
        _ => Vec::new(),
    };
    let line = match vm.get_prop_raw(e, b"line") {
        Value::Int(n) => n as u32,
        _ => 0,
    };
    (cls, file, line)
}

// ------------------------------------------------------------ hooks

/// Stops: the header, the listing, then commands until one resumes.
fn stop_at(vm: &mut Vm, header: Option<&str>, file: &[u8], line: u32, ret_marker: bool) -> R<()> {
    if let Some(h) = header {
        msg(vm, h);
    }
    listing(vm, file, line, 3, true);
    with(|d| {
        d.in_hook = true;
        d.cur = Some(Pos {
            file: file.to_vec(),
            line,
            ret_marker,
        });
    });
    let flow = command_loop(vm);
    with(|d| {
        d.in_hook = false;
        d.cur = None;
    });
    match flow {
        Flow::Resume | Flow::Run(..) => Ok(()),
        Flow::Abort(end) => {
            with(|d| d.abort = Some(end));
            Err(Unwind::Fatal)
        }
    }
}

/// The Zend opcode name of an instruction (for opcode breakpoints).
fn zend_name(op: &Op) -> &'static str {
    use crate::bytecode::IncDec;
    match op {
        Op::Echo { .. } => "ZEND_ECHO",
        Op::IncDecVar { op, .. } => match op {
            IncDec::PreInc => "ZEND_PRE_INC",
            IncDec::PreDec => "ZEND_PRE_DEC",
            IncDec::PostInc => "ZEND_POST_INC",
            IncDec::PostDec => "ZEND_POST_DEC",
        },
        Op::Throw { .. } => "ZEND_THROW",
        Op::Return { .. } | Op::ReturnRef { .. } => "ZEND_RETURN",
        Op::GeneratorReturn { .. } => "ZEND_GENERATOR_RETURN",
        Op::Assign { .. } | Op::AssignRes { .. } => "ZEND_ASSIGN",
        Op::AssignDim { .. } => "ZEND_ASSIGN_DIM",
        Op::AssignProp { .. } => "ZEND_ASSIGN_OBJ",
        Op::AssignRef { .. } => "ZEND_ASSIGN_REF",
        Op::Add { .. } => "ZEND_ADD",
        Op::Sub { .. } => "ZEND_SUB",
        Op::Mul { .. } => "ZEND_MUL",
        Op::Concat { .. } | Op::Rope { .. } => "ZEND_CONCAT",
        Op::InitFCall { .. } => "ZEND_INIT_FCALL",
        Op::InitMethod { .. } => "ZEND_INIT_METHOD_CALL",
        Op::InitStaticCall { .. } => "ZEND_INIT_STATIC_METHOD_CALL",
        Op::InitDynCall { .. } => "ZEND_INIT_DYNAMIC_CALL",
        Op::DoCall { .. } => "ZEND_DO_FCALL",
        Op::New { .. } => "ZEND_NEW",
        Op::Include { .. } | Op::Eval { .. } => "ZEND_INCLUDE_OR_EVAL",
        Op::Exit { .. } => "ZEND_EXIT",
        Op::Jmp { .. } => "ZEND_JMP",
        Op::JmpZ { .. } => "ZEND_JMPZ",
        Op::JmpNZ { .. } => "ZEND_JMPNZ",
        Op::FeReset { .. } => "ZEND_FE_RESET_R",
        Op::FeFetch { .. } => "ZEND_FE_FETCH_R",
        Op::Yield { .. } => "ZEND_YIELD",
        Op::YieldFrom { .. } => "ZEND_YIELD_FROM",
        Op::Catch { .. } => "ZEND_CATCH",
        Op::FetchDim { .. } | Op::FetchDim2 { .. } => "ZEND_FETCH_DIM_R",
        Op::FetchProp { .. } => "ZEND_FETCH_OBJ_R",
        Op::Clone { .. } => "ZEND_CLONE",
        Op::UnsetVar { .. } => "ZEND_UNSET_CV",
        Op::Isset { .. } | Op::IssetVar { .. } => "ZEND_ISSET_ISEMPTY_CV",
        Op::Cast { .. } => "ZEND_CAST",
        Op::DeclareFunction { .. } => "ZEND_DECLARE_FUNCTION",
        Op::DeclareClass { .. } => "ZEND_DECLARE_CLASS",
        Op::MakeClosure { .. } => "ZEND_DECLARE_LAMBDA_FUNCTION",
        Op::Send { .. } | Op::SendVar { .. } => "ZEND_SEND_VAL",
        _ => "",
    }
}

fn lower(b: &[u8]) -> Vec<u8> {
    b.strip_prefix(b"\\").unwrap_or(b).to_ascii_lowercase()
}

/// Whether a breakpoint's file is this code's file (a relative name
/// matches the end of the path).
fn file_matches(bp: &[u8], file: &[u8]) -> bool {
    if bp == file {
        return true;
    }
    if !bp.starts_with(b"/") && file.len() > bp.len() {
        let tail = &file[file.len() - bp.len()..];
        return tail == bp && file[file.len() - bp.len() - 1] == b'/';
    }
    false
}

/// At a statement marker, the instruction pointer past it. An error
/// unwinds the script.
#[cold]
#[inline(never)]
pub(crate) fn on_marker(vm: &mut Vm) -> R<()> {
    if with(|d| d.in_hook || !d.running) {
        return Ok(());
    }
    let depth = vm.ex.frames.len();
    let codep = vm.ex.top.code;
    if codep.is_null() {
        return Ok(());
    }
    // SAFETY: the running frame's code.
    let code: &FuncCode = unsafe { &*codep };
    if code.file.is_empty() || code.flags & fflags::CONST_EXPR != 0 {
        return Ok(());
    }
    let ip = vm.ex.ip().saturating_sub(1);
    let line = code.line_of(ip);
    let cid = codep as usize;
    let next = code.ops.get(ip + 1).copied();
    let ret_marker = matches!(
        next,
        Some(Op::Return { .. } | Op::ReturnRef { .. } | Op::GeneratorReturn { .. })
    );
    // The end of a finally block with an exception pending rethrows it:
    // the stop is the exception's (test sapi/phpdbg/tests/exceptions_003).
    if let Some(Op::FastRet { try_index, .. }) = next
        && let Some(tr) = code.try_ranges.get(try_index as usize)
    {
        let p = vm.ex.top.base;
        // SAFETY: a slot of the running frame.
        let pending = unsafe {
            matches!(
                &*p.add(crate::bytecode::slot_of(tr.exc_slot)),
                Some(Value::Object(_))
            )
        };
        if pending {
            return Ok(());
        }
    }
    let file = code.file.to_vec();
    let first = !code.ops[..ip].iter().any(|o| matches!(o, Op::Ticks));
    let need_ops = with(|d| {
        d.bps
            .iter()
            .any(|b| matches!(b.kind, BpKind::Opcode { .. }))
    });
    let ops: Vec<&'static str> = if need_ops {
        code.ops[ip + 1..]
            .iter()
            .take_while(|o| !matches!(o, Op::Ticks))
            .map(zend_name)
            .collect()
    } else {
        Vec::new()
    };
    let fname = lower(&code.name);
    let cname = code
        .class_name
        .as_ref()
        .filter(|_| code.flags & fflags::CLOSURE == 0)
        .map(|c| lower(c));
    let watch_msg = watch::check(vm);
    if with(|d| d.abort.is_some()) {
        return Err(Unwind::Fatal);
    }
    let (header, stop) = with(|d| {
        let same_line = d.last_marker == (depth, cid, line);
        d.last_marker = (depth, cid, line);
        let mut header: Option<String> = None;
        for b in d.bps.iter_mut() {
            let hit = match &mut b.kind {
                BpKind::Next { addr, done } if !*done => {
                    *done = true;
                    Some(format!("{:#x}", addr))
                }
                BpKind::Line { file: f, line: l } if *l == line && !same_line => {
                    file_matches(f, &file).then(String::new)
                }
                BpKind::Func { lname, shown } if first && cname.is_none() && *lname == fname => {
                    Some(format!("{}()", shown))
                }
                BpKind::Method {
                    class,
                    method,
                    shown,
                } if first && cname.as_ref() == Some(class) && *method == fname => {
                    Some(format!("{}()", shown))
                }
                BpKind::Opcode { name } if !same_line && ops.contains(&name.as_str()) => {
                    Some(name.clone())
                }
                _ => None,
            };
            if let Some(what) = hit {
                b.hits += 1;
                let place = format!("{}:{}", String::from_utf8_lossy(&file), line);
                header = Some(if what.is_empty() {
                    format!("[Breakpoint #{} at {}, hits: {}]", b.id, place, b.hits)
                } else {
                    format!(
                        "[Breakpoint #{} in {} at {}, hits: {}]",
                        b.id, what, place, b.hits
                    )
                });
                break;
            }
        }
        if header.is_none() {
            header = watch_msg;
        }
        if header.is_some() {
            d.step = None;
            return (header, true);
        }
        let stop = match d.step {
            Some(Step::Into {
                depth: d0,
                code: c0,
                line: l0,
            }) => (depth, cid, line) != (d0, c0, l0),
            Some(Step::Over {
                depth: d0,
                code: c0,
                line: l0,
            }) => depth < d0 || (depth == d0 && (cid, line) != (c0, l0)),
            Some(Step::Finish { depth: d0 }) => depth < d0,
            Some(Step::Leave { depth: d0 }) => {
                if depth < d0 || (depth == d0 && ret_marker) {
                    d.step = None;
                    return (
                        Some(format!(
                            "[Breaking for leave at {}:{}]",
                            String::from_utf8_lossy(&file),
                            line
                        )),
                        true,
                    );
                }
                false
            }
            None => false,
        };
        if stop {
            d.step = None;
        }
        (None, stop)
    });
    if !stop {
        return Ok(());
    }
    stop_at(vm, header.as_deref(), &file, line, ret_marker)
}

/// Whether a handler will catch the exception: a matching catch or a
/// finally block around the current instruction of some frame.
fn will_be_caught(vm: &Vm, ex: &ObjRef) -> bool {
    let cls = vm.class_of(ex);
    // The running fiber's frames, then those of the code that resumed it.
    std::iter::once(&vm.ex)
        .chain(vm.ex_parents.iter().rev())
        .any(|state| {
            let n = state.frames.len();
            (0..n).rev().any(|i| {
                let f = &state.frames[i];
                let ip = if i == n - 1 {
                    state.ip().saturating_sub(1)
                } else {
                    f.ip().saturating_sub(1)
                } as u32;
                frame_catches(vm, f.code(), ip, &cls)
            })
        })
}

fn frame_catches(vm: &Vm, code: &FuncCode, ip: u32, cls: &crate::class::Class) -> bool {
    for tr in &code.try_ranges {
        if ip < tr.start {
            continue;
        }
        if ip < tr.end && tr.catch != 0 {
            let mut at = tr.catch as usize;
            while let Some(Op::Catch { list, next, .. }) = code.ops.get(at) {
                for &k in &code.lists[*list as usize] {
                    let k = (k & !crate::bytecode::CONST) as usize;
                    if let Some(Value::Str(name)) = code.consts.get(k)
                        && vm.instance_of_lname(cls, &lower(name))
                    {
                        return true;
                    }
                }
                if *next as usize <= at {
                    break;
                }
                at = *next as usize;
            }
        }
        if tr.finally != 0 && ip < tr.finally {
            return true;
        }
    }
    false
}

/// Each time the dispatcher looks for a handler of an exception in a
/// frame. An error unwinds the script.
#[cold]
#[inline(never)]
pub(crate) fn on_exception(vm: &mut Vm, ex: &ObjRef) -> R<()> {
    if with(|d| d.in_hook || !d.running) {
        return Ok(());
    }
    let depth = vm.ex.frames.len();
    if depth == 0 {
        return Ok(());
    }
    let codep = vm.ex.top.code;
    if codep.is_null() {
        return Ok(());
    }
    // SAFETY: the running frame's code.
    let code: &FuncCode = unsafe { &*codep };
    let ip = vm.ex.ip().saturating_sub(1);
    let line = code.line_of(ip);
    let file = code.file.to_vec();
    let origin = with(|d| {
        d.seen_ex.retain(|e| e.0.upgrade().is_some());
        let found = d
            .seen_ex
            .iter()
            .find(|e| e.0.upgrade().is_some_and(|o| ObjRef::ptr_eq(&o, ex)))
            .map(|e| (e.1, e.2));
        if found.is_none() {
            d.seen_ex.push((ex.downgrade(), depth, line));
        }
        found
    });
    // Destroying a suspended generator unwinds it with an internal
    // exception that is not the script's.
    let internal = ex.class_name().as_bytes();
    if internal == b"GeneratorExit_" {
        return Ok(());
    }
    if origin.is_none() && !will_be_caught(vm, ex) {
        let (mut cls, mut efile, mut eline) = exception_place(vm, ex);
        // A destroyed fiber's unwinding has no place of its own: the stop
        // shows where the fiber is (test sapi/phpdbg/tests/gh13827.phpt).
        if internal == b"FiberExit_" {
            cls = "GracefulExit".into();
        }
        let shown_file = String::from_utf8_lossy(&efile).into_owned();
        let shown_line = eline;
        if efile.is_empty() {
            efile = file.clone();
            eline = line;
        }
        let m = match vm.get_prop_raw(ex, b"message") {
            Value::Str(s) => s.to_string_lossy(),
            _ => String::new(),
        };
        let header = format!(
            "[Uncaught {} in {} on line {}: {}]",
            cls, shown_file, shown_line, m
        );
        with(|d| d.step = None);
        return stop_at(vm, Some(&header), &efile, eline, false);
    }
    let stop = with(|d| match d.step {
        Some(Step::Into { .. }) => true,
        Some(Step::Over { depth: d0, .. }) => depth <= d0,
        _ => false,
    });
    if !stop || code.file.is_empty() {
        return Ok(());
    }
    let sline = match origin {
        Some((od, ol)) if od == depth => ol,
        _ => line,
    };
    with(|d| d.step = None);
    stop_at(vm, None, &file, sline, false)
}

// ------------------------------------------------------------ command line

/// What `goro --phpdbg` was given (PHP manual: phpdbg, Command line
/// options).
#[derive(Default)]
pub struct Options {
    pub quiet: bool,
    pub ignore_init: bool,
    pub init_file: Option<Vec<u8>>,
    pub run: bool,
    pub stdin: bool,
    pub script: Option<Vec<u8>>,
    pub args: Vec<Vec<u8>>,
    pub ini: Vec<(Vec<u8>, Vec<u8>)>,
}

pub const USAGE: &str = "Usage: goro --phpdbg [options] [file] [args...]
  -c <path>|<file> Look for php.ini file in this directory (ignored)
  -d foo[=bar]     Define INI entry foo with value 'bar'
  -n               No php.ini file will be used
  -q               Do not print the banner
  -b               Disable colour
  -i <file>        Override the .phpdbginit file
  -I               Ignore the default .phpdbginit
  -r               Run the execution context
  -s               Read the code to execute from stdin
  -e               (accepted, no effect)
  -h               This help

Commands come from stdin (type \"help\").
";

/// Parses the phpdbg command line.
pub fn parse_args(args: &[Vec<u8>]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == b"--" {
            i += 1;
            break;
        }
        if !a.starts_with(b"-") || a.len() < 2 {
            break;
        }
        let mut k = 1;
        while k < a.len() {
            let c = a[k];
            if b"cdizSOlapx".contains(&c) {
                let v = if k + 1 < a.len() {
                    a[k + 1..].to_vec()
                } else {
                    i += 1;
                    match args.get(i) {
                        Some(v) => v.clone(),
                        None => return Err(format!("option -{} needs a value", c as char)),
                    }
                };
                match c {
                    b'd' => {
                        let (n, val) = match v.iter().position(|&b| b == b'=') {
                            Some(p) => (v[..p].to_vec(), v[p + 1..].to_vec()),
                            None => (v.clone(), b"1".to_vec()),
                        };
                        o.ini.push((n, val));
                    }
                    b'i' => o.init_file = Some(v),
                    _ => {}
                }
                break;
            }
            match c {
                b'q' => o.quiet = true,
                b'I' => o.ignore_init = true,
                b'r' => o.run = true,
                b's' => o.stdin = true,
                b'h' => return Err(String::new()),
                b'b' | b'n' | b'e' | b'E' | b'v' | b'V' => {}
                _ => return Err(format!("unknown option -{}", c as char)),
            }
            k += 1;
        }
        i += 1;
    }
    if i < args.len() {
        o.script = Some(args[i].clone());
        o.args = args[i + 1..].to_vec();
    }
    Ok(o)
}

/// Commands of an init file (`.phpdbginit`): one per line, `#` lines are
/// comments.
fn init_commands(path: &std::path::Path) -> Vec<String> {
    let Ok(t) = std::fs::read(path) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&t)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Runs phpdbg; returns the exit status.
pub fn main(o: Options) -> i32 {
    crate::compiler::DEBUG_MARKERS.with(|c| c.set(true));
    // Commands are read from a copy of stdin: `run < file` swaps the
    // script's stdin only.
    // SAFETY: plain descriptor duplication; the copy is owned by the File.
    let input: Box<dyn BufRead> = unsafe {
        use std::os::unix::io::FromRawFd;
        let fd = libc::dup(0);
        if fd < 0 {
            Box::new(std::io::BufReader::new(std::io::empty()))
        } else {
            Box::new(std::io::BufReader::new(std::fs::File::from_raw_fd(fd)))
        }
    };
    with(|d| {
        d.prompt = "prompt> ".into();
        d.ini = o.ini.clone();
        d.args = o.args.clone();
        d.input = Some(input);
        d.line_start = true;
    });
    let mut idle = make_vm(&o.ini);
    if !o.quiet {
        write_raw(
            b"[Welcome to phpdbg, the interactive PHP debugger, v8.5.4]\n\
              To get help using phpdbg type \"help\" and press enter\n\
              [Please report bugs to <https://github.com/php/php-src/issues>]\n",
        );
    }
    // Init files: -i FILE, else .phpdbginit in the working directory and
    // in the ini scan directories (tests sapi/phpdbg/tests/bug73615.phpt,
    // gh12962.phpt).
    let mut init = Vec::new();
    if let Some(f) = &o.init_file {
        use std::os::unix::ffi::OsStrExt;
        init.extend(init_commands(std::path::Path::new(
            std::ffi::OsStr::from_bytes(f),
        )));
    } else if !o.ignore_init {
        let mut seen = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from(".")];
        if let Some(scan) = std::env::var_os("PHP_INI_SCAN_DIR") {
            for d in std::env::split_paths(&scan) {
                if !d.as_os_str().is_empty() {
                    dirs.push(d);
                }
            }
        }
        for d in dirs {
            let p = d.join(".phpdbginit");
            let c = std::fs::canonicalize(&p).unwrap_or(p.clone());
            if p.is_file() && !seen.contains(&c) {
                seen.push(c);
                init.extend(init_commands(&p));
            }
        }
    }
    if let Some(s) = &o.script {
        set_context(&mut idle, s, false);
    } else if o.stdin {
        let mut src = Vec::new();
        with(|d| {
            if let Some(i) = d.input.as_mut() {
                let _ = i.read_to_end(&mut src);
            }
        });
        commands::set_stdin_context(&mut idle, src);
    }
    with(|d| {
        d.queue.extend(init);
        if o.run {
            d.queue.push_back("run".into());
        }
    });
    loop {
        if let Flow::Run(args, stdin) = command_loop(&mut idle) {
            let mut next = Some((args, stdin));
            while let Some((a, s)) = next.take() {
                if let RunEnd::Restart(a2, s2) = do_run(a, s) {
                    next = Some((a2, s2));
                }
            }
            with(|d| d.seen_written = idle.out.written);
        }
    }
}
