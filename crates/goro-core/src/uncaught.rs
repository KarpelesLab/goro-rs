//! Reporting of uncaught exceptions / fatal errors at the end of the main
//! script (zend_exception_error + php_error_cb display), written through the
//! output layer so output handlers see it, before shutdown functions run.

use crate::array::ArrayKey;
use crate::string::PhpString;
use crate::value::Value;
use crate::vm::{Vm, VmError};
use std::rc::Rc;

impl Vm {
    /// Format an exception's "trace" property like Exception::getTraceAsString().
    pub fn format_exception_trace_value(
        trace_val: &Value,
        fallback_trace: &str,
        max_len: usize,
    ) -> String {
        if let Value::Array(arr) = trace_val {
            let arr = arr.borrow();
            let mut lines = Vec::new();
            let mut idx = 0;
            let key = |k: &[u8]| ArrayKey::String(PhpString::from_bytes(k));
            for (_key, frame_val) in arr.iter() {
                if let Value::Array(frame) = frame_val {
                    let frame = frame.borrow();
                    // Appended as C strings (smart_str_appends): an anonymous
                    // class name ends at its NUL byte.
                    let get_s = |k: &[u8]| {
                        frame
                            .get(&key(k))
                            .map(|v| {
                                let s = v.to_php_string();
                                let b = s.as_bytes();
                                let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
                                String::from_utf8_lossy(&b[..end]).into_owned()
                            })
                            .unwrap_or_default()
                    };
                    let file = get_s(b"file");
                    let line = frame.get(&key(b"line")).map(|v| v.to_long()).unwrap_or(0);
                    let function = get_s(b"function");
                    let class = get_s(b"class");
                    let type_str = get_s(b"type");
                    let args_str = if let Some(Value::Array(args_arr)) = frame.get(&key(b"args")) {
                        let args_arr = args_arr.borrow();
                        let formatted: Vec<String> = args_arr
                            .iter()
                            .map(|(_k, v)| Vm::format_trace_arg_with_limit(v, max_len))
                            .collect();
                        formatted.join(", ")
                    } else {
                        String::new()
                    };
                    let loc = if file.is_empty() || file == "[internal function]" || line == 0 {
                        "[internal function]".to_string()
                    } else {
                        format!("{}({})", file, line)
                    };
                    lines.push(format!(
                        "#{} {}: {}{}{}({})",
                        idx, loc, class, type_str, function, args_str
                    ));
                }
                idx += 1;
            }
            lines.push(format!("#{} {{main}}", idx));
            return lines.join("\n");
        }
        fallback_trace.to_string()
    }

    /// Display an uncaught exception (or a not-yet-displayed fatal error)
    /// through the output layer. Clears `current_exception` and sets
    /// `fatal_emitted` so front-ends do not print it again.
    pub fn report_uncaught(&mut self, e: &VmError) {
        let stack_trace = self.format_stack_trace();
        let max_len = self.exception_string_param_max_len;
        if let Some(exc) = self.current_exception.take() {
            // zend_exception_error(): the report is what the exception's
            // (possibly user-defined) __toString() returns.
            if let Value::Object(obj) = &exc {
                let cl = obj.borrow().class_name.to_ascii_lowercase();
                let user_tostring = self.classes.get(&cl).is_some_and(|c| {
                    c.get_method(b"__tostring").is_some_and(|m| {
                        !m.op_array.ops.is_empty()
                            && !m.declaring_class.eq_ignore_ascii_case(b"exception")
                            && !m.declaring_class.eq_ignore_ascii_case(b"error")
                    })
                });
                if user_tostring {
                    let r = self.call_object_method(&exc, b"__tostring", &[]);
                    if self.current_exception.is_none() {
                        if let Some(s) = r.map(|v| v.deref()).and_then(|v| match v {
                            Value::String(s) => Some(s.to_string_lossy()),
                            _ => None,
                        }) {
                            let (file, line) = {
                                let o = obj.borrow();
                                (
                                    o.get_property(b"file").to_php_string().to_string_lossy(),
                                    o.get_property(b"line").to_long() as u32,
                                )
                            };
                            let fatal = format!("Uncaught {}\n  thrown", s);
                            self.display_error("Fatal error", &fatal, &file, line);
                            self.fatal_emitted = true;
                            return;
                        }
                    }
                    self.current_exception = None;
                }
            }
            // zend_exception_error(): an uncaught ParseError / CompileError
            // (exactly those classes) is reported as E_PARSE / E_COMPILE_ERROR
            // with its own message, file and line.
            if let Value::Object(obj) = &exc {
                let cl = obj.borrow().class_name.to_ascii_lowercase();
                if cl == b"parseerror" || cl == b"compileerror" {
                    let (file, line, msg) = {
                        let o = obj.borrow();
                        (
                            o.get_property(b"file").to_php_string().to_string_lossy(),
                            o.get_property(b"line").to_long() as u32,
                            o.get_property(b"message").deref(),
                        )
                    };
                    let msg = match self.value_to_string_checked(&msg) {
                        Ok(s) => s.to_string_lossy(),
                        Err(_) => String::new(),
                    };
                    self.current_exception = None;
                    let ty = if cl == b"parseerror" {
                        crate::errdisplay::E_PARSE
                    } else {
                        crate::errdisplay::E_COMPILE_ERROR
                    };
                    self.fatal_error_cb_ex(ty, &msg, &file, line, false);
                    self.fatal_emitted = true;
                    return;
                }
            }
            // Messages may be objects with __toString().
            let mut msg_strings: Vec<(usize, String)> = Vec::new();
            {
                let mut cur = exc.clone();
                for _ in 0..100 {
                    let Value::Object(o) = &cur else { break };
                    let msg = o.borrow().get_property(b"message");
                    if matches!(msg.deref(), Value::Object(_)) {
                        if let Ok(s) = self.value_to_string_checked(&msg.deref()) {
                            msg_strings.push((Rc::as_ptr(o) as usize, s.to_string_lossy()));
                        }
                        self.current_exception = None;
                    }
                    let next = o.borrow().get_property(b"previous");
                    cur = next;
                }
            }
            let msg_of = |o: &std::rc::Rc<std::cell::RefCell<crate::object::PhpObject>>| {
                msg_strings
                    .iter()
                    .find(|(p, _)| *p == Rc::as_ptr(o) as usize)
                    .map(|(_, s)| s.clone())
                    .unwrap_or_else(|| {
                        o.borrow()
                            .get_property(b"message")
                            .to_php_string()
                            .to_string_lossy()
                    })
            };
            if let Value::Object(obj) = &exc {
                let mut chain = Vec::new();
                {
                    let obj_ref = obj.borrow();
                    let class = crate::value::display_class_name(&obj_ref.class_name);
                    drop(obj_ref);
                    let msg_str = msg_of(obj);
                    let obj_ref = obj.borrow();
                    let has_file = obj_ref.has_property(b"file");
                    let has_line = obj_ref.has_property(b"line");
                    let exc_file = obj_ref
                        .get_property(b"file")
                        .to_php_string()
                        .to_string_lossy();
                    let exc_line = obj_ref.get_property(b"line").to_long();
                    let file_str = if has_file {
                        exc_file
                    } else {
                        self.current_file.clone()
                    };
                    let line_num = if has_line && exc_line >= 0 {
                        exc_line as u32
                    } else {
                        e.line
                    };
                    let exc_trace = obj_ref.get_property(b"trace");
                    let trace_str =
                        Self::format_exception_trace_value(&exc_trace, &stack_trace, max_len);
                    chain.push((class, msg_str, file_str, line_num, trace_str));
                    let mut prev = obj_ref.get_property(b"previous").clone();
                    for _ in 0..100 {
                        let next_prev;
                        if let Value::Object(prev_obj) = &prev {
                            let prev_ref = prev_obj.borrow();
                            let pc = crate::value::display_class_name(&prev_ref.class_name);
                            drop(prev_ref);
                            let pm = msg_of(prev_obj);
                            let prev_ref = prev_obj.borrow();
                            let pf = prev_ref
                                .get_property(b"file")
                                .to_php_string()
                                .to_string_lossy();
                            let pl = prev_ref.get_property(b"line").to_long();
                            let pf_str = if pf.is_empty() || pf == "Unknown" {
                                self.current_file.clone()
                            } else {
                                pf
                            };
                            let pl_num = if pl > 0 { pl as u32 } else { 0 };
                            let pt = prev_ref.get_property(b"trace");
                            let pt_str =
                                Self::format_exception_trace_value(&pt, &stack_trace, max_len);
                            chain.push((pc, pm, pf_str, pl_num, pt_str));
                            next_prev = prev_ref.get_property(b"previous").clone();
                        } else {
                            break;
                        }
                        prev = next_prev;
                    }
                }
                let first = &chain[0];
                let thrown_file = if first.2.is_empty() {
                    "Unknown".to_string()
                } else {
                    first.2.clone()
                };
                let thrown_line = first.3;
                let mut fatal = String::new();
                for (i, (class, msg_str, file_str, line_num, trace_str)) in
                    chain.iter().rev().enumerate()
                {
                    let head = if i == 0 { "Uncaught " } else { "\n\nNext " };
                    if msg_str.is_empty() {
                        fatal.push_str(&format!(
                            "{}{} in {}:{}\nStack trace:\n{}",
                            head, class, file_str, line_num, trace_str
                        ));
                    } else {
                        fatal.push_str(&format!(
                            "{}{}: {}{}{}:{}\nStack trace:\n{}",
                            head,
                            class,
                            msg_str,
                            Vm::exception_location_connector(msg_str),
                            file_str,
                            line_num,
                            trace_str
                        ));
                    }
                }
                fatal.push_str("\n  thrown");
                self.fatal_error_cb(
                    crate::errdisplay::E_ERROR,
                    &fatal,
                    &thrown_file,
                    thrown_line,
                );
                self.fatal_emitted = true;
                return;
            }
        }
        if !self.fatal_emitted {
            let file = self.current_file.clone();
            self.fatal_error_cb(crate::errdisplay::E_ERROR, &e.message, &file, e.line);
            self.fatal_emitted = true;
        }
    }
}
