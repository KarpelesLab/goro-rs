use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

// ── Thread-local parser storage ─────────────────────────────────────────────

thread_local! {
    static XML_PARSERS: RefCell<HashMap<i64, XmlParser>> = RefCell::new(HashMap::new());
    static NEXT_PARSER_ID: Cell<i64> = const { Cell::new(1) };
}

// ── XmlParser struct ────────────────────────────────────────────────────────

struct XmlParser {
    start_handler: Option<Value>,
    end_handler: Option<Value>,
    char_data_handler: Option<Value>,
    default_handler: Option<Value>,
    pi_handler: Option<Value>,
    case_folding: bool,
    target_encoding: String,
    current_byte_index: usize,
    current_line: u32,
    current_column: u32,
    error_code: i64,
    parser_object: Option<Value>,
    separator: Option<String>,
}

impl XmlParser {
    fn new() -> Self {
        Self {
            start_handler: None,
            end_handler: None,
            char_data_handler: None,
            default_handler: None,
            pi_handler: None,
            case_folding: true,
            target_encoding: "UTF-8".to_string(),
            current_byte_index: 0,
            current_line: 1,
            current_column: 0,
            error_code: 0,
            parser_object: None,
            separator: None,
        }
    }

}

// ── XML error constants ─────────────────────────────────────────────────────

const XML_ERROR_NONE: i64 = 0;
const XML_ERROR_SYNTAX: i64 = 2;
const XML_ERROR_INVALID_TOKEN: i64 = 4;
const XML_ERROR_TAG_MISMATCH: i64 = 7;

// XML option constants
const XML_OPTION_CASE_FOLDING: i64 = 1;
const XML_OPTION_TARGET_ENCODING: i64 = 2;
const XML_OPTION_SKIP_TAGSTART: i64 = 3;
const XML_OPTION_SKIP_WHITE: i64 = 4;

// ── SAX parser events ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum XmlEvent {
    StartElement {
        name: String,
        attributes: Vec<(String, String)>,
    },
    EndElement {
        name: String,
    },
    CharacterData {
        data: String,
    },
    ProcessingInstruction {
        target: String,
        data: String,
    },
    Default {
        data: String,
    },
}

// ── SAX parser state machine ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum ParseState {
    Text,
    TagOpen,        // just saw '<'
    TagName,        // reading tag name
    EndTagName,     // reading </tagname
    AttrSpace,      // whitespace between attrs
    AttrName,       // reading attribute name
    AttrEq,         // expecting '='
    AttrValueStart, // expecting opening quote
    AttrValueQuoted, // reading "value"
    SelfClose,      // saw '/' in a tag, expect '>'
    Comment1,       // saw <!-
    Comment,        // inside <!-- ... -->
    CommentDash1,   // saw - inside comment
    CommentDash2,   // saw -- inside comment
    Cdata1,         // saw <![
    Cdata2,         // saw <![C
    Cdata3,         // saw <![CD
    Cdata4,         // saw <![CDA
    Cdata5,         // saw <![CDAT
    Cdata6,         // saw <![CDATA
    CdataContent,   // inside <![CDATA[ ... ]]>
    CdataClose1,    // saw ]
    CdataClose2,    // saw ]]
    #[allow(dead_code)]
    Pi,             // inside <?...?>
    PiTarget,       // reading PI target name
    PiData,         // reading PI data
    PiClose,        // saw ? in PI
    Doctype,        // inside <!DOCTYPE ...>
    BangTag,        // saw <! (deciding comment/cdata/doctype)
    XmlDecl,        // inside <?xml ... ?> declaration
}

fn decode_entity(entity: &str) -> Option<String> {
    match entity {
        "amp" => Some("&".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "apos" => Some("'".to_string()),
        "quot" => Some("\"".to_string()),
        _ => {
            if entity.starts_with('#') {
                let code = if entity.starts_with("#x") || entity.starts_with("#X") {
                    u32::from_str_radix(&entity[2..], 16).ok()
                } else {
                    entity[1..].parse::<u32>().ok()
                };
                code.and_then(char::from_u32).map(|c| c.to_string())
            } else {
                None
            }
        }
    }
}

fn resolve_entities(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            let mut entity = String::new();
            let mut found_semi = false;
            for ec in chars.by_ref() {
                if ec == ';' {
                    found_semi = true;
                    break;
                }
                entity.push(ec);
                if entity.len() > 10 {
                    break;
                }
            }
            if found_semi {
                if let Some(decoded) = decode_entity(&entity) {
                    result.push_str(&decoded);
                } else {
                    // Unknown entity - pass through
                    result.push('&');
                    result.push_str(&entity);
                    result.push(';');
                }
            } else {
                result.push('&');
                result.push_str(&entity);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse XML data into a list of events.
/// Returns Ok(events) on success or Err((error_code, line, column, byte_index)) on failure.
fn parse_xml(
    data: &str,
    start_byte: usize,
    start_line: u32,
    start_col: u32,
    ns_separator: Option<&str>,
) -> Result<(Vec<XmlEvent>, usize, u32, u32), (i64, u32, u32, usize)> {
    let mut events: Vec<XmlEvent> = Vec::new();
    let mut state = ParseState::Text;
    let mut text_buf = String::new();
    let mut tag_name = String::new();
    let mut attr_name = String::new();
    let mut attr_value = String::new();
    let mut attr_quote = '"';
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut pi_target = String::new();
    let mut pi_data = String::new();
    let mut default_buf = String::new();
    let mut cdata_buf = String::new();
    let mut line = start_line;
    let mut col = start_col;
    let mut byte_index = start_byte;
    let mut tag_stack: Vec<String> = Vec::new();

    // Process namespace separator in tag names
    let process_ns = |name: &str| -> String {
        if let Some(sep) = ns_separator {
            // Replace ':' in tag names with the namespace separator
            name.replacen(':', sep, 1)
        } else {
            name.to_string()
        }
    };

    let bytes = data.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i] as char;

        // Track position
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }

        match state {
            ParseState::Text => {
                if c == '<' {
                    if !text_buf.is_empty() {
                        let resolved = resolve_entities(&text_buf);
                        events.push(XmlEvent::CharacterData {
                            data: resolved,
                        });
                        text_buf.clear();
                    }
                    state = ParseState::TagOpen;
                    default_buf.clear();
                    default_buf.push('<');
                } else {
                    text_buf.push(c);
                }
            }
            ParseState::TagOpen => {
                if c == '/' {
                    state = ParseState::EndTagName;
                    tag_name.clear();
                    default_buf.push('/');
                } else if c == '?' {
                    state = ParseState::PiTarget;
                    pi_target.clear();
                    pi_data.clear();
                    default_buf.push('?');
                } else if c == '!' {
                    state = ParseState::BangTag;
                    default_buf.push('!');
                } else if c.is_ascii_alphabetic() || c == '_' || c == ':' {
                    state = ParseState::TagName;
                    tag_name.clear();
                    tag_name.push(c);
                    attrs.clear();
                    default_buf.push(c);
                } else {
                    return Err((XML_ERROR_INVALID_TOKEN, line, col, byte_index + i));
                }
            }
            ParseState::BangTag => {
                if c == '-' {
                    state = ParseState::Comment1;
                    default_buf.push(c);
                } else if c == '[' {
                    state = ParseState::Cdata1;
                    default_buf.push(c);
                } else if c.is_ascii_alphabetic() {
                    // DOCTYPE or similar
                    state = ParseState::Doctype;
                    default_buf.push(c);
                } else {
                    return Err((XML_ERROR_INVALID_TOKEN, line, col, byte_index + i));
                }
            }
            ParseState::Comment1 => {
                if c == '-' {
                    state = ParseState::Comment;
                    default_buf.push(c);
                } else {
                    return Err((XML_ERROR_INVALID_TOKEN, line, col, byte_index + i));
                }
            }
            ParseState::Comment => {
                default_buf.push(c);
                if c == '-' {
                    state = ParseState::CommentDash1;
                }
            }
            ParseState::CommentDash1 => {
                default_buf.push(c);
                if c == '-' {
                    state = ParseState::CommentDash2;
                } else {
                    state = ParseState::Comment;
                }
            }
            ParseState::CommentDash2 => {
                default_buf.push(c);
                if c == '>' {
                    // End of comment
                    events.push(XmlEvent::Default {
                        data: default_buf.clone(),
                    });
                    default_buf.clear();
                    state = ParseState::Text;
                } else if c == '-' {
                    // Stay in CommentDash2 (multiple dashes)
                } else {
                    state = ParseState::Comment;
                }
            }
            ParseState::Cdata1 => {
                default_buf.push(c);
                if c == 'C' {
                    state = ParseState::Cdata2;
                } else {
                    return Err((XML_ERROR_INVALID_TOKEN, line, col, byte_index + i));
                }
            }
            ParseState::Cdata2 => {
                default_buf.push(c);
                if c == 'D' {
                    state = ParseState::Cdata3;
                } else {
                    return Err((XML_ERROR_INVALID_TOKEN, line, col, byte_index + i));
                }
            }
            ParseState::Cdata3 => {
                default_buf.push(c);
                if c == 'A' {
                    state = ParseState::Cdata4;
                } else {
                    return Err((XML_ERROR_INVALID_TOKEN, line, col, byte_index + i));
                }
            }
            ParseState::Cdata4 => {
                default_buf.push(c);
                if c == 'T' {
                    state = ParseState::Cdata5;
                } else {
                    return Err((XML_ERROR_INVALID_TOKEN, line, col, byte_index + i));
                }
            }
            ParseState::Cdata5 => {
                default_buf.push(c);
                if c == 'A' {
                    state = ParseState::Cdata6;
                } else {
                    return Err((XML_ERROR_INVALID_TOKEN, line, col, byte_index + i));
                }
            }
            ParseState::Cdata6 => {
                default_buf.push(c);
                if c == '[' {
                    state = ParseState::CdataContent;
                    cdata_buf.clear();
                    default_buf.clear(); // CDATA content is treated as character data
                } else {
                    return Err((XML_ERROR_INVALID_TOKEN, line, col, byte_index + i));
                }
            }
            ParseState::CdataContent => {
                if c == ']' {
                    state = ParseState::CdataClose1;
                } else {
                    cdata_buf.push(c);
                }
            }
            ParseState::CdataClose1 => {
                if c == ']' {
                    state = ParseState::CdataClose2;
                } else {
                    cdata_buf.push(']');
                    cdata_buf.push(c);
                    state = ParseState::CdataContent;
                }
            }
            ParseState::CdataClose2 => {
                if c == '>' {
                    // End of CDATA
                    events.push(XmlEvent::CharacterData {
                        data: cdata_buf.clone(),
                    });
                    cdata_buf.clear();
                    state = ParseState::Text;
                } else if c == ']' {
                    // Extra ']', stay in CdataClose2 but add one ']' to buffer
                    cdata_buf.push(']');
                } else {
                    cdata_buf.push(']');
                    cdata_buf.push(']');
                    cdata_buf.push(c);
                    state = ParseState::CdataContent;
                }
            }
            ParseState::Doctype => {
                default_buf.push(c);
                if c == '>' {
                    events.push(XmlEvent::Default {
                        data: default_buf.clone(),
                    });
                    default_buf.clear();
                    state = ParseState::Text;
                }
            }
            ParseState::PiTarget => {
                if c.is_ascii_whitespace() {
                    if pi_target.eq_ignore_ascii_case("xml") {
                        state = ParseState::XmlDecl;
                        default_buf.push(c);
                    } else {
                        state = ParseState::PiData;
                    }
                } else if c == '?' {
                    state = ParseState::PiClose;
                } else {
                    pi_target.push(c);
                    default_buf.push(c);
                }
            }
            ParseState::XmlDecl => {
                default_buf.push(c);
                if c == '>' && default_buf.ends_with("?>") {
                    events.push(XmlEvent::Default {
                        data: default_buf.clone(),
                    });
                    default_buf.clear();
                    state = ParseState::Text;
                }
            }
            ParseState::PiData => {
                if c == '?' {
                    state = ParseState::PiClose;
                } else {
                    pi_data.push(c);
                }
            }
            ParseState::PiClose => {
                if c == '>' {
                    // Strip leading whitespace from PI data
                    let trimmed_data = pi_data.trim_start().to_string();
                    events.push(XmlEvent::ProcessingInstruction {
                        target: pi_target.clone(),
                        data: trimmed_data,
                    });
                    pi_target.clear();
                    pi_data.clear();
                    state = ParseState::Text;
                } else {
                    pi_data.push('?');
                    pi_data.push(c);
                    state = ParseState::PiData;
                }
            }
            ParseState::TagName => {
                if c == '>' {
                    let processed = process_ns(&tag_name);
                    tag_stack.push(processed.clone());
                    events.push(XmlEvent::StartElement {
                        name: processed,
                        attributes: attrs.clone(),
                    });
                    attrs.clear();
                    tag_name.clear();
                    state = ParseState::Text;
                } else if c == '/' {
                    state = ParseState::SelfClose;
                } else if c.is_ascii_whitespace() {
                    state = ParseState::AttrSpace;
                } else {
                    tag_name.push(c);
                }
            }
            ParseState::AttrSpace => {
                if c == '>' {
                    let processed = process_ns(&tag_name);
                    tag_stack.push(processed.clone());
                    events.push(XmlEvent::StartElement {
                        name: processed,
                        attributes: attrs.clone(),
                    });
                    attrs.clear();
                    tag_name.clear();
                    state = ParseState::Text;
                } else if c == '/' {
                    state = ParseState::SelfClose;
                } else if !c.is_ascii_whitespace() {
                    attr_name.clear();
                    attr_name.push(c);
                    state = ParseState::AttrName;
                }
            }
            ParseState::AttrName => {
                if c == '=' {
                    state = ParseState::AttrValueStart;
                } else if c.is_ascii_whitespace() {
                    state = ParseState::AttrEq;
                } else {
                    attr_name.push(c);
                }
            }
            ParseState::AttrEq => {
                if c == '=' {
                    state = ParseState::AttrValueStart;
                } else if !c.is_ascii_whitespace() {
                    return Err((XML_ERROR_SYNTAX, line, col, byte_index + i));
                }
            }
            ParseState::AttrValueStart => {
                if c == '"' || c == '\'' {
                    attr_quote = c;
                    attr_value.clear();
                    state = ParseState::AttrValueQuoted;
                } else if !c.is_ascii_whitespace() {
                    return Err((XML_ERROR_SYNTAX, line, col, byte_index + i));
                }
            }
            ParseState::AttrValueQuoted => {
                if c == attr_quote {
                    let resolved_val = resolve_entities(&attr_value);
                    let processed_name = process_ns(&attr_name);
                    attrs.push((processed_name, resolved_val));
                    attr_name.clear();
                    attr_value.clear();
                    state = ParseState::AttrSpace;
                } else {
                    attr_value.push(c);
                }
            }
            ParseState::SelfClose => {
                if c == '>' {
                    let processed = process_ns(&tag_name);
                    events.push(XmlEvent::StartElement {
                        name: processed.clone(),
                        attributes: attrs.clone(),
                    });
                    events.push(XmlEvent::EndElement {
                        name: processed,
                    });
                    attrs.clear();
                    tag_name.clear();
                    state = ParseState::Text;
                } else {
                    return Err((XML_ERROR_SYNTAX, line, col, byte_index + i));
                }
            }
            ParseState::EndTagName => {
                if c == '>' {
                    let processed = process_ns(&tag_name);
                    // Check tag mismatch
                    if let Some(expected) = tag_stack.pop() {
                        if !expected.eq_ignore_ascii_case(&processed) {
                            return Err((XML_ERROR_TAG_MISMATCH, line, col, byte_index + i));
                        }
                    }
                    events.push(XmlEvent::EndElement {
                        name: processed,
                    });
                    tag_name.clear();
                    state = ParseState::Text;
                } else if c.is_ascii_whitespace() {
                    // Allow whitespace before '>' in end tags
                } else {
                    tag_name.push(c);
                }
            }
            ParseState::Pi => {
                // Unreachable: Pi state is not entered in normal flow
                // (we go directly to PiTarget from TagOpen)
                state = ParseState::Text;
            }
        }

        i += 1;
        byte_index = start_byte + i;
    }

    // Handle trailing text
    if !text_buf.is_empty() {
        let resolved = resolve_entities(&text_buf);
        events.push(XmlEvent::CharacterData { data: resolved });
    }

    Ok((events, byte_index, line, col))
}

// ── Callback dispatch ───────────────────────────────────────────────────────

/// Call a PHP callback (function name string, array callback, or callable Value).
fn call_callback(
    vm: &mut Vm,
    callback: &Value,
    args: Vec<Value>,
) -> Result<Value, VmError> {
    match callback {
        Value::String(s) => {
            let func_name = s.as_bytes();
            let func_lower: Vec<u8> = func_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            if let Some(builtin) = vm.functions.get(&func_lower).copied() {
                return builtin(vm, &args);
            }
            if let Some(user_fn) = vm.user_functions.get(&func_lower).cloned() {
                return vm.execute_fn_with_named_args(&user_fn, args, vec![], None);
            }
            Ok(Value::Null)
        }
        Value::Array(arr) => {
            let arr_borrow = arr.borrow();
            let vals: Vec<Value> = arr_borrow.values().cloned().collect();
            drop(arr_borrow);
            if vals.len() >= 2 {
                let method_name = vals[1].to_php_string();
                let method_lower: Vec<u8> =
                    method_name.as_bytes().iter().map(|b| b.to_ascii_lowercase()).collect();
                if let Value::Object(obj) = &vals[0] {
                    let class_lower: Vec<u8> = {
                        let obj_borrow = obj.borrow();
                        obj_borrow
                            .class_name
                            .iter()
                            .map(|b| b.to_ascii_lowercase())
                            .collect()
                    };
                    if let Some(class) = vm.classes.get(&class_lower).cloned() {
                        if let Some(method) = class.methods.get(&method_lower) {
                            let op = method.op_array.clone();
                            return vm.execute_fn_with_named_args(
                                &op,
                                args,
                                vec![],
                                Some(Value::Object(obj.clone())),
                            );
                        }
                    }
                }
            }
            Ok(Value::Null)
        }
        _ => Ok(Value::Null),
    }
}

/// Dispatch a SAX event callback.
/// If `parser_object` is set, the first argument is that object.
/// Otherwise the first argument is the parser resource ID.
fn dispatch_callback(
    vm: &mut Vm,
    callback: &Value,
    parser_id: i64,
    parser_object: &Option<Value>,
    mut extra_args: Vec<Value>,
) -> Result<(), VmError> {
    // If the callback is a string and the parser has an object set, use method dispatch
    let first_arg = parser_object
        .clone()
        .unwrap_or(Value::Long(parser_id));

    let effective_callback = if let (Value::String(method_name), Some(obj)) =
        (callback, parser_object)
    {
        // Build [object, method_name] array callback
        let mut arr = PhpArray::new();
        arr.push(obj.clone());
        arr.push(Value::String(method_name.clone()));
        Value::Array(Rc::new(RefCell::new(arr)))
    } else {
        callback.clone()
    };

    let mut args = vec![first_arg];
    args.append(&mut extra_args);
    call_callback(vm, &effective_callback, args)?;
    Ok(())
}

// ── Public registration ─────────────────────────────────────────────────────

pub fn register(vm: &mut Vm) {
    vm.register_extension(b"xml");
    // Functions
    vm.register_function(b"xml_parser_create", xml_parser_create);
    vm.register_function(b"xml_parser_create_ns", xml_parser_create_ns);
    vm.register_function(b"xml_parse", xml_parse);
    vm.register_function(b"xml_parser_free", xml_parser_free);
    vm.register_function(b"xml_set_element_handler", xml_set_element_handler);
    vm.register_function(b"xml_set_character_data_handler", xml_set_character_data_handler);
    vm.register_function(b"xml_set_default_handler", xml_set_default_handler);
    vm.register_function(
        b"xml_set_processing_instruction_handler",
        xml_set_processing_instruction_handler,
    );
    vm.register_function(b"xml_parser_set_option", xml_parser_set_option);
    vm.register_function(b"xml_parser_get_option", xml_parser_get_option);
    vm.register_function(b"xml_set_object", xml_set_object);
    vm.register_function(b"xml_get_error_code", xml_get_error_code);
    vm.register_function(b"xml_error_string", xml_error_string);
    vm.register_function(b"xml_get_current_line_number", xml_get_current_line_number);
    vm.register_function(b"xml_get_current_column_number", xml_get_current_column_number);
    vm.register_function(b"xml_get_current_byte_index", xml_get_current_byte_index);
    vm.register_function(b"xml_parse_into_struct", xml_parse_into_struct);
    vm.register_function(b"xml_set_external_entity_ref_handler", xml_set_stub_handler);
    vm.register_function(b"xml_set_notation_decl_handler", xml_set_stub_handler);
    vm.register_function(b"xml_set_unparsed_entity_decl_handler", xml_set_stub_handler);
    vm.register_function(b"xml_set_start_namespace_decl_handler", xml_set_stub_handler);
    vm.register_function(b"xml_set_end_namespace_decl_handler", xml_set_stub_handler);
    vm.register_function(b"simplexml_load_string", simplexml_load_string);
    vm.register_function(b"simplexml_load_file", simplexml_load_file);

    // Constants
    vm.constants
        .insert(b"XML_ERROR_NONE".to_vec(), Value::Long(0));
    vm.constants
        .insert(b"XML_ERROR_NO_MEMORY".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"XML_ERROR_SYNTAX".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"XML_ERROR_NO_ELEMENTS".to_vec(), Value::Long(3));
    vm.constants
        .insert(b"XML_ERROR_INVALID_TOKEN".to_vec(), Value::Long(4));
    vm.constants
        .insert(b"XML_ERROR_UNCLOSED_TOKEN".to_vec(), Value::Long(5));
    vm.constants
        .insert(b"XML_ERROR_PARTIAL_CHAR".to_vec(), Value::Long(6));
    vm.constants
        .insert(b"XML_ERROR_TAG_MISMATCH".to_vec(), Value::Long(7));
    vm.constants
        .insert(b"XML_ERROR_DUPLICATE_ATTRIBUTE".to_vec(), Value::Long(8));
    vm.constants.insert(
        b"XML_ERROR_JUNK_AFTER_DOC_ELEMENT".to_vec(),
        Value::Long(9),
    );
    vm.constants
        .insert(b"XML_ERROR_PARAM_ENTITY_REF".to_vec(), Value::Long(10));
    vm.constants
        .insert(b"XML_ERROR_UNDEFINED_ENTITY".to_vec(), Value::Long(11));
    vm.constants
        .insert(b"XML_ERROR_RECURSIVE_ENTITY_REF".to_vec(), Value::Long(12));
    vm.constants
        .insert(b"XML_ERROR_ASYNC_ENTITY".to_vec(), Value::Long(13));
    vm.constants
        .insert(b"XML_ERROR_BAD_CHAR_REF".to_vec(), Value::Long(14));
    vm.constants
        .insert(b"XML_ERROR_BINARY_ENTITY_REF".to_vec(), Value::Long(15));
    vm.constants.insert(
        b"XML_ERROR_ATTRIBUTE_EXTERNAL_ENTITY_REF".to_vec(),
        Value::Long(16),
    );
    vm.constants
        .insert(b"XML_ERROR_MISPLACED_XML_PI".to_vec(), Value::Long(17));
    vm.constants
        .insert(b"XML_ERROR_UNKNOWN_ENCODING".to_vec(), Value::Long(18));
    vm.constants
        .insert(b"XML_ERROR_INCORRECT_ENCODING".to_vec(), Value::Long(19));
    vm.constants.insert(
        b"XML_ERROR_UNCLOSED_CDATA_SECTION".to_vec(),
        Value::Long(20),
    );
    vm.constants.insert(
        b"XML_ERROR_EXTERNAL_ENTITY_HANDLING".to_vec(),
        Value::Long(21),
    );
    vm.constants
        .insert(b"XML_OPTION_CASE_FOLDING".to_vec(), Value::Long(1));
    vm.constants
        .insert(b"XML_OPTION_TARGET_ENCODING".to_vec(), Value::Long(2));
    vm.constants
        .insert(b"XML_OPTION_SKIP_TAGSTART".to_vec(), Value::Long(3));
    vm.constants
        .insert(b"XML_OPTION_SKIP_WHITE".to_vec(), Value::Long(4));
    vm.constants.insert(
        b"XML_SAX_IMPL".to_vec(),
        Value::String(PhpString::from_bytes(b"goro")),
    );
}

// ── Built-in function implementations ───────────────────────────────────────

fn xml_parser_create(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _encoding = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();

    let id = NEXT_PARSER_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    });

    let parser = XmlParser::new();
    XML_PARSERS.with(|p| p.borrow_mut().insert(id, parser));

    Ok(Value::Long(id))
}

fn xml_parser_create_ns(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _encoding = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();

    let separator = args
        .get(1)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| ":".to_string());

    let id = NEXT_PARSER_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    });

    let mut parser = XmlParser::new();
    parser.separator = Some(separator);
    XML_PARSERS.with(|p| p.borrow_mut().insert(id, parser));

    Ok(Value::Long(id))
}

fn xml_parse(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let data = args
        .get(1)
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    // Extract parser state for parsing
    let (start_byte, start_line, start_col, separator, case_folding) = XML_PARSERS.with(|p| {
        let parsers = p.borrow();
        if let Some(parser) = parsers.get(&parser_id) {
            Ok((
                parser.current_byte_index,
                parser.current_line,
                parser.current_column,
                parser.separator.clone(),
                parser.case_folding,
            ))
        } else {
            Err(VmError {
                message: "xml_parse(): supplied argument is not a valid XML Parser resource"
                    .to_string(),
                line: 0,
            })
        }
    })?;

    let sep_ref = separator.as_deref();
    let parse_result = parse_xml(&data, start_byte, start_line, start_col, sep_ref);

    match parse_result {
        Ok((events, end_byte, end_line, end_col)) => {
            // Update parser position
            XML_PARSERS.with(|p| {
                let mut parsers = p.borrow_mut();
                if let Some(parser) = parsers.get_mut(&parser_id) {
                    parser.current_byte_index = end_byte;
                    parser.current_line = end_line;
                    parser.current_column = end_col;
                    parser.error_code = XML_ERROR_NONE;
                }
            });

            // Now dispatch events - extract handlers from thread local
            for event in events {
                let (handler, parser_obj) = XML_PARSERS.with(|p| {
                    let parsers = p.borrow();
                    let parser = parsers.get(&parser_id).unwrap();
                    let handler = match &event {
                        XmlEvent::StartElement { .. } => parser.start_handler.clone(),
                        XmlEvent::EndElement { .. } => parser.end_handler.clone(),
                        XmlEvent::CharacterData { .. } => parser.char_data_handler.clone(),
                        XmlEvent::ProcessingInstruction { .. } => parser.pi_handler.clone(),
                        XmlEvent::Default { .. } => parser.default_handler.clone(),
                    };
                    (handler, parser.parser_object.clone())
                });

                match event {
                    XmlEvent::StartElement { name, attributes } => {
                        if let Some(ref cb) = handler {
                            let folded_name = if case_folding {
                                name.to_ascii_uppercase()
                            } else {
                                name
                            };
                            let mut attr_array = PhpArray::new();
                            for (k, v) in attributes {
                                let folded_key = if case_folding {
                                    k.to_ascii_uppercase()
                                } else {
                                    k
                                };
                                attr_array.set(
                                    ArrayKey::String(PhpString::from_string(folded_key)),
                                    Value::String(PhpString::from_string(v)),
                                );
                            }
                            dispatch_callback(
                                vm,
                                cb,
                                parser_id,
                                &parser_obj,
                                vec![
                                    Value::String(PhpString::from_string(folded_name)),
                                    Value::Array(Rc::new(RefCell::new(attr_array))),
                                ],
                            )?;
                        } else {
                            // No start handler - check default handler
                            let default_h = XML_PARSERS.with(|p| {
                                let parsers = p.borrow();
                                parsers
                                    .get(&parser_id)
                                    .and_then(|parser| parser.default_handler.clone())
                            });
                            if let Some(ref cb) = default_h {
                                let mut tag_str = format!("<{}", name);
                                for (k, v) in attributes {
                                    tag_str.push_str(&format!(" {}=\"{}\"", k, v));
                                }
                                tag_str.push('>');
                                dispatch_callback(
                                    vm,
                                    cb,
                                    parser_id,
                                    &parser_obj,
                                    vec![Value::String(PhpString::from_string(tag_str))],
                                )?;
                            }
                        }
                    }
                    XmlEvent::EndElement { name } => {
                        if let Some(ref cb) = handler {
                            let folded_name = if case_folding {
                                name.to_ascii_uppercase()
                            } else {
                                name
                            };
                            dispatch_callback(
                                vm,
                                cb,
                                parser_id,
                                &parser_obj,
                                vec![Value::String(PhpString::from_string(folded_name))],
                            )?;
                        } else {
                            let default_h = XML_PARSERS.with(|p| {
                                let parsers = p.borrow();
                                parsers
                                    .get(&parser_id)
                                    .and_then(|parser| parser.default_handler.clone())
                            });
                            if let Some(ref cb) = default_h {
                                let tag_str = format!("</{}>", name);
                                dispatch_callback(
                                    vm,
                                    cb,
                                    parser_id,
                                    &parser_obj,
                                    vec![Value::String(PhpString::from_string(tag_str))],
                                )?;
                            }
                        }
                    }
                    XmlEvent::CharacterData { data: cdata } => {
                        if let Some(ref cb) = handler {
                            dispatch_callback(
                                vm,
                                cb,
                                parser_id,
                                &parser_obj,
                                vec![Value::String(PhpString::from_string(cdata))],
                            )?;
                        } else {
                            let default_h = XML_PARSERS.with(|p| {
                                let parsers = p.borrow();
                                parsers
                                    .get(&parser_id)
                                    .and_then(|parser| parser.default_handler.clone())
                            });
                            if let Some(ref cb) = default_h {
                                dispatch_callback(
                                    vm,
                                    cb,
                                    parser_id,
                                    &parser_obj,
                                    vec![Value::String(PhpString::from_string(cdata))],
                                )?;
                            }
                        }
                    }
                    XmlEvent::ProcessingInstruction {
                        target,
                        data: pi_data,
                    } => {
                        if let Some(ref cb) = handler {
                            dispatch_callback(
                                vm,
                                cb,
                                parser_id,
                                &parser_obj,
                                vec![
                                    Value::String(PhpString::from_string(target)),
                                    Value::String(PhpString::from_string(pi_data)),
                                ],
                            )?;
                        } else {
                            let default_h = XML_PARSERS.with(|p| {
                                let parsers = p.borrow();
                                parsers
                                    .get(&parser_id)
                                    .and_then(|parser| parser.default_handler.clone())
                            });
                            if let Some(ref cb) = default_h {
                                let pi_str = format!("<?{} {}?>", target, pi_data);
                                dispatch_callback(
                                    vm,
                                    cb,
                                    parser_id,
                                    &parser_obj,
                                    vec![Value::String(PhpString::from_string(pi_str))],
                                )?;
                            }
                        }
                    }
                    XmlEvent::Default { data: def_data } => {
                        if let Some(ref cb) = handler {
                            dispatch_callback(
                                vm,
                                cb,
                                parser_id,
                                &parser_obj,
                                vec![Value::String(PhpString::from_string(def_data))],
                            )?;
                        }
                    }
                }
            }

            Ok(Value::Long(1))
        }
        Err((err_code, err_line, err_col, err_byte)) => {
            XML_PARSERS.with(|p| {
                let mut parsers = p.borrow_mut();
                if let Some(parser) = parsers.get_mut(&parser_id) {
                    parser.error_code = err_code;
                    parser.current_line = err_line;
                    parser.current_column = err_col;
                    parser.current_byte_index = err_byte;
                }
            });
            Ok(Value::Long(0))
        }
    }
}

fn xml_parser_free(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let removed = XML_PARSERS.with(|p| p.borrow_mut().remove(&parser_id).is_some());
    Ok(if removed { Value::True } else { Value::False })
}

fn xml_set_element_handler(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let start = args.get(1).cloned().unwrap_or(Value::Null);
    let end = args.get(2).cloned().unwrap_or(Value::Null);

    XML_PARSERS.with(|p| {
        let mut parsers = p.borrow_mut();
        if let Some(parser) = parsers.get_mut(&parser_id) {
            parser.start_handler = if start.is_truthy() || matches!(&start, Value::String(_)) {
                Some(to_callback_value(&start))
            } else {
                None
            };
            parser.end_handler = if end.is_truthy() || matches!(&end, Value::String(_)) {
                Some(to_callback_value(&end))
            } else {
                None
            };
            Value::True
        } else {
            Value::False
        }
    });

    Ok(Value::True)
}

fn xml_set_character_data_handler(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let handler = args.get(1).cloned().unwrap_or(Value::Null);

    XML_PARSERS.with(|p| {
        let mut parsers = p.borrow_mut();
        if let Some(parser) = parsers.get_mut(&parser_id) {
            parser.char_data_handler = if handler.is_truthy() || matches!(&handler, Value::String(_))
            {
                Some(to_callback_value(&handler))
            } else {
                None
            };
        }
    });

    Ok(Value::True)
}

fn xml_set_default_handler(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let handler = args.get(1).cloned().unwrap_or(Value::Null);

    XML_PARSERS.with(|p| {
        let mut parsers = p.borrow_mut();
        if let Some(parser) = parsers.get_mut(&parser_id) {
            parser.default_handler = if handler.is_truthy() || matches!(&handler, Value::String(_))
            {
                Some(to_callback_value(&handler))
            } else {
                None
            };
        }
    });

    Ok(Value::True)
}

fn xml_set_processing_instruction_handler(
    _vm: &mut Vm,
    args: &[Value],
) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let handler = args.get(1).cloned().unwrap_or(Value::Null);

    XML_PARSERS.with(|p| {
        let mut parsers = p.borrow_mut();
        if let Some(parser) = parsers.get_mut(&parser_id) {
            parser.pi_handler = if handler.is_truthy() || matches!(&handler, Value::String(_)) {
                Some(to_callback_value(&handler))
            } else {
                None
            };
        }
    });

    Ok(Value::True)
}

fn xml_parser_set_option(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let option = args.get(1).unwrap_or(&Value::Null).to_long();
    let value = args.get(2).cloned().unwrap_or(Value::Null);

    XML_PARSERS.with(|p| {
        let mut parsers = p.borrow_mut();
        if let Some(parser) = parsers.get_mut(&parser_id) {
            match option {
                XML_OPTION_CASE_FOLDING => {
                    parser.case_folding = value.is_truthy();
                    Ok(Value::True)
                }
                XML_OPTION_TARGET_ENCODING => {
                    parser.target_encoding = value.to_php_string().to_string_lossy();
                    Ok(Value::True)
                }
                XML_OPTION_SKIP_TAGSTART | XML_OPTION_SKIP_WHITE => {
                    // Accepted but not actively used
                    Ok(Value::True)
                }
                _ => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn xml_parser_get_option(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let option = args.get(1).unwrap_or(&Value::Null).to_long();

    XML_PARSERS.with(|p| {
        let parsers = p.borrow();
        if let Some(parser) = parsers.get(&parser_id) {
            match option {
                XML_OPTION_CASE_FOLDING => {
                    Ok(Value::Long(if parser.case_folding { 1 } else { 0 }))
                }
                XML_OPTION_TARGET_ENCODING => Ok(Value::String(PhpString::from_string(
                    parser.target_encoding.clone(),
                ))),
                XML_OPTION_SKIP_TAGSTART => Ok(Value::Long(0)),
                XML_OPTION_SKIP_WHITE => Ok(Value::Long(0)),
                _ => Ok(Value::False),
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn xml_set_object(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let object = args.get(1).cloned().unwrap_or(Value::Null);

    XML_PARSERS.with(|p| {
        let mut parsers = p.borrow_mut();
        if let Some(parser) = parsers.get_mut(&parser_id) {
            parser.parser_object = if matches!(&object, Value::Object(_)) {
                Some(object)
            } else {
                None
            };
        }
    });

    Ok(Value::True)
}

fn xml_get_error_code(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let code = XML_PARSERS.with(|p| {
        let parsers = p.borrow();
        parsers.get(&parser_id).map(|pp| pp.error_code).unwrap_or(0)
    });
    Ok(Value::Long(code))
}

fn xml_error_string(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let code = args.first().unwrap_or(&Value::Null).to_long();
    let msg = match code {
        0 => "No error",
        1 => "No memory",
        2 => "syntax error",
        3 => "no element found",
        4 => "not well-formed (invalid token)",
        5 => "unclosed token",
        6 => "Invalid character",
        7 => "mismatched tag",
        8 => "duplicate attribute",
        9 => "junk after document element",
        10 => "illegal parameter entity reference",
        11 => "undefined entity",
        12 => "recursive entity reference",
        13 => "asynchronous entity",
        14 => "reference to invalid character number",
        15 => "reference to binary entity",
        16 => "reference to external entity in attribute",
        17 => "XML or TEXT declaration not at start of entity",
        18 => "unknown encoding",
        19 => "encoding specified in XML declaration is incorrect",
        20 => "unclosed CDATA section",
        21 => "error in processing external entity reference",
        _ => "Unknown error",
    };
    Ok(Value::String(PhpString::from_string(msg.to_string())))
}

fn xml_get_current_line_number(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let line = XML_PARSERS.with(|p| {
        let parsers = p.borrow();
        parsers
            .get(&parser_id)
            .map(|pp| pp.current_line as i64)
            .unwrap_or(0)
    });
    Ok(Value::Long(line))
}

fn xml_get_current_column_number(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let col = XML_PARSERS.with(|p| {
        let parsers = p.borrow();
        parsers
            .get(&parser_id)
            .map(|pp| pp.current_column as i64)
            .unwrap_or(0)
    });
    Ok(Value::Long(col))
}

fn xml_get_current_byte_index(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let idx = XML_PARSERS.with(|p| {
        let parsers = p.borrow();
        parsers
            .get(&parser_id)
            .map(|pp| pp.current_byte_index as i64)
            .unwrap_or(-1)
    });
    Ok(Value::Long(idx))
}

fn xml_parse_into_struct(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let parser_id = args.first().unwrap_or(&Value::Null).to_long();
    let data = args
        .get(1)
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    // Extract parser state
    let (separator, case_folding) = XML_PARSERS.with(|p| {
        let parsers = p.borrow();
        if let Some(parser) = parsers.get(&parser_id) {
            Ok((parser.separator.clone(), parser.case_folding))
        } else {
            Err(VmError {
                message: "xml_parse_into_struct(): supplied argument is not a valid XML Parser resource".to_string(),
                line: 0,
            })
        }
    })?;

    let sep_ref = separator.as_deref();
    let parse_result = parse_xml(&data, 0, 1, 0, sep_ref);

    match parse_result {
        Ok((events, _, _, _)) => {
            let mut values_arr = PhpArray::new();
            let mut index_arr = PhpArray::new();
            let mut depth: usize = 0;
            let mut value_idx: i64 = 0;

            for event in &events {
                match event {
                    XmlEvent::StartElement { name, attributes } => {
                        depth += 1;
                        let folded = if case_folding {
                            name.to_ascii_uppercase()
                        } else {
                            name.clone()
                        };
                        let mut entry = PhpArray::new();
                        entry.set(
                            ArrayKey::String(PhpString::from_bytes(b"tag")),
                            Value::String(PhpString::from_string(folded.clone())),
                        );
                        entry.set(
                            ArrayKey::String(PhpString::from_bytes(b"type")),
                            Value::String(PhpString::from_bytes(b"open")),
                        );
                        entry.set(
                            ArrayKey::String(PhpString::from_bytes(b"level")),
                            Value::Long(depth as i64),
                        );
                        if !attributes.is_empty() {
                            let mut attr_arr = PhpArray::new();
                            for (k, v) in attributes {
                                let folded_key = if case_folding {
                                    k.to_ascii_uppercase()
                                } else {
                                    k.clone()
                                };
                                attr_arr.set(
                                    ArrayKey::String(PhpString::from_string(folded_key)),
                                    Value::String(PhpString::from_string(v.clone())),
                                );
                            }
                            entry.set(
                                ArrayKey::String(PhpString::from_bytes(b"attributes")),
                                Value::Array(Rc::new(RefCell::new(attr_arr))),
                            );
                        }
                        values_arr.push(Value::Array(Rc::new(RefCell::new(entry))));

                        // Update index
                        let tag_key = ArrayKey::String(PhpString::from_string(folded));
                        if let Some(existing) = index_arr.get(&tag_key) {
                            if let Value::Array(arr) = existing {
                                arr.borrow_mut().push(Value::Long(value_idx));
                            }
                        } else {
                            let mut idx_entry = PhpArray::new();
                            idx_entry.push(Value::Long(value_idx));
                            index_arr.set(
                                tag_key,
                                Value::Array(Rc::new(RefCell::new(idx_entry))),
                            );
                        }
                        value_idx += 1;
                    }
                    XmlEvent::EndElement { name } => {
                        let folded = if case_folding {
                            name.to_ascii_uppercase()
                        } else {
                            name.clone()
                        };

                        // Check if the previous entry for this tag was an "open" with no value/children
                        // If so, change it to "complete" instead of adding a separate "close"
                        let mut merged = false;
                        if value_idx > 0 {
                            let prev_key = ArrayKey::Int(value_idx - 1);
                            if let Some(Value::Array(prev_arr)) = values_arr.get(&prev_key) {
                                let prev = prev_arr.borrow();
                                let prev_type = prev
                                    .get(&ArrayKey::String(PhpString::from_bytes(b"type")))
                                    .map(|v| v.to_php_string().to_string_lossy())
                                    .unwrap_or_default();
                                let prev_tag = prev
                                    .get(&ArrayKey::String(PhpString::from_bytes(b"tag")))
                                    .map(|v| v.to_php_string().to_string_lossy())
                                    .unwrap_or_default();
                                if prev_type == "open" && prev_tag == folded {
                                    merged = true;
                                }
                                drop(prev);
                                if merged {
                                    if let Some(Value::Array(prev_arr)) =
                                        values_arr.get(&prev_key)
                                    {
                                        prev_arr.borrow_mut().set(
                                            ArrayKey::String(PhpString::from_bytes(b"type")),
                                            Value::String(PhpString::from_bytes(b"complete")),
                                        );
                                    }
                                }
                            }
                        }

                        if !merged {
                            let mut entry = PhpArray::new();
                            entry.set(
                                ArrayKey::String(PhpString::from_bytes(b"tag")),
                                Value::String(PhpString::from_string(folded.clone())),
                            );
                            entry.set(
                                ArrayKey::String(PhpString::from_bytes(b"type")),
                                Value::String(PhpString::from_bytes(b"close")),
                            );
                            entry.set(
                                ArrayKey::String(PhpString::from_bytes(b"level")),
                                Value::Long(depth as i64),
                            );
                            values_arr.push(Value::Array(Rc::new(RefCell::new(entry))));

                            let tag_key =
                                ArrayKey::String(PhpString::from_string(folded));
                            if let Some(existing) = index_arr.get(&tag_key) {
                                if let Value::Array(arr) = existing {
                                    arr.borrow_mut().push(Value::Long(value_idx));
                                }
                            } else {
                                let mut idx_entry = PhpArray::new();
                                idx_entry.push(Value::Long(value_idx));
                                index_arr.set(
                                    tag_key,
                                    Value::Array(Rc::new(RefCell::new(idx_entry))),
                                );
                            }
                            value_idx += 1;
                        }

                        if depth > 0 {
                            depth -= 1;
                        }
                    }
                    XmlEvent::CharacterData { data: cdata } => {
                        // Append value to the most recent open element
                        if value_idx > 0 {
                            let prev_key = ArrayKey::Int(value_idx - 1);
                            if let Some(Value::Array(prev_arr)) = values_arr.get(&prev_key) {
                                let has_value = prev_arr
                                    .borrow()
                                    .get(&ArrayKey::String(PhpString::from_bytes(b"value")))
                                    .is_some();
                                if has_value {
                                    // Append to existing value
                                    let old = prev_arr
                                        .borrow()
                                        .get(&ArrayKey::String(PhpString::from_bytes(b"value")))
                                        .unwrap()
                                        .to_php_string()
                                        .to_string_lossy();
                                    let combined = format!("{}{}", old, cdata);
                                    prev_arr.borrow_mut().set(
                                        ArrayKey::String(PhpString::from_bytes(b"value")),
                                        Value::String(PhpString::from_string(combined)),
                                    );
                                } else {
                                    prev_arr.borrow_mut().set(
                                        ArrayKey::String(PhpString::from_bytes(b"value")),
                                        Value::String(PhpString::from_string(cdata.clone())),
                                    );
                                }
                            }
                        }
                    }
                    _ => {
                        // PI, Default events are ignored in parse_into_struct
                    }
                }
            }

            // Set the values array (arg 2) and index array (arg 3) by reference
            if let Some(values_ref) = args.get(2) {
                if let Value::Reference(r) = values_ref {
                    *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(values_arr)));
                }
            }
            if let Some(index_ref) = args.get(3) {
                if let Value::Reference(r) = index_ref {
                    *r.borrow_mut() = Value::Array(Rc::new(RefCell::new(index_arr)));
                }
            }

            Ok(Value::Long(1))
        }
        Err((err_code, _, _, _)) => {
            XML_PARSERS.with(|p| {
                let mut parsers = p.borrow_mut();
                if let Some(parser) = parsers.get_mut(&parser_id) {
                    parser.error_code = err_code;
                }
            });
            Ok(Value::Long(0))
        }
    }
}

// ── Helper: convert a value to a callback-storable form ─────────────────────

fn to_callback_value(val: &Value) -> Value {
    match val {
        Value::String(_) => val.clone(),
        Value::Array(_) => val.clone(),
        Value::Object(_) => val.clone(),
        _ => {
            let s = val.to_php_string().to_string_lossy();
            Value::String(PhpString::from_string(s))
        }
    }
}

// ── SimpleXML ───────────────────────────────────────────────────────────────

fn build_simplexml_subtree(
    vm: &mut Vm,
    events: &[XmlEvent],
    pos: &mut usize,
    tag_name: &str,
    tag_attrs: &[(String, String)],
) -> Value {
    let mut children: HashMap<String, Vec<Value>> = HashMap::new();
    let mut text_content = String::new();
    let mut child_order: Vec<String> = Vec::new();

    while *pos < events.len() {
        match &events[*pos] {
            XmlEvent::StartElement {
                name,
                attributes: attrs,
            } => {
                let child_name = name.clone();
                let child_attrs = attrs.clone();
                *pos += 1;
                let child_obj =
                    build_simplexml_subtree(vm, events, pos, &child_name, &child_attrs);

                if !child_order.contains(&child_name) {
                    child_order.push(child_name.clone());
                }
                children.entry(child_name).or_default().push(child_obj);
            }
            XmlEvent::EndElement { .. } => {
                *pos += 1;
                break;
            }
            XmlEvent::CharacterData { data } => {
                text_content.push_str(data);
                *pos += 1;
            }
            _ => {
                *pos += 1;
            }
        }
    }

    let obj_id = vm.next_object_id();
    let mut obj = PhpObject::new(b"SimpleXMLElement".to_vec(), obj_id);

    // Store the tag name as an internal property (for getName())
    obj.set_property(b"__sxml_name".to_vec(), Value::String(PhpString::from_string(tag_name.to_string())));

    // Set attributes
    if !tag_attrs.is_empty() {
        let mut attr_arr = PhpArray::new();
        for (k, v) in tag_attrs {
            attr_arr.set(
                ArrayKey::String(PhpString::from_string(k.clone())),
                Value::String(PhpString::from_string(v.clone())),
            );
        }
        obj.set_property(
            b"@attributes".to_vec(),
            Value::Array(Rc::new(RefCell::new(attr_arr))),
        );
    }

    if children.is_empty() {
        // Leaf node: if has non-whitespace text, the object stringifies to the text
        // We store it as "0" property (internal) for __toString
        // PHP SimpleXML does not create text nodes for whitespace-only content
        if !text_content.is_empty() && text_content.chars().any(|c| !c.is_whitespace()) {
            obj.set_property(b"0".to_vec(), Value::String(PhpString::from_string(text_content)));
        }
    } else {
        for key in &child_order {
            let vals = children.get(key).unwrap();
            if vals.len() == 1 {
                obj.set_property(key.as_bytes().to_vec(), vals[0].clone());
            } else {
                let mut arr = PhpArray::new();
                for v in vals {
                    arr.push(v.clone());
                }
                obj.set_property(key.as_bytes().to_vec(), Value::Array(Rc::new(RefCell::new(arr))));
            }
        }
    }

    Value::Object(Rc::new(RefCell::new(obj)))
}

fn simplexml_load_string(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    if data.trim().is_empty() {
        return Ok(Value::False);
    }

    // Parse without case folding for SimpleXML (preserves original case)
    let parse_result = parse_xml(&data, 0, 1, 0, None);
    match parse_result {
        Ok((events, _, _, _)) => {
            if events.is_empty() {
                return Ok(Value::False);
            }

            // Find the root element
            let mut pos = 0;
            // Skip xml declarations and whitespace
            while pos < events.len() {
                if matches!(&events[pos], XmlEvent::StartElement { .. }) {
                    break;
                }
                pos += 1;
            }

            if pos >= events.len() {
                return Ok(Value::False);
            }

            // Extract root element info
            let (root_name, root_attrs) = if let XmlEvent::StartElement {
                name, attributes, ..
            } = &events[pos]
            {
                (name.clone(), attributes.clone())
            } else {
                return Ok(Value::False);
            };
            pos += 1;

            let root = build_simplexml_subtree(vm, &events, &mut pos, &root_name, &root_attrs);
            Ok(root)
        }
        Err(_) => Ok(Value::False),
    }
}

fn simplexml_load_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();

    match std::fs::read_to_string(&filename) {
        Ok(contents) => {
            let data_val = Value::String(PhpString::from_string(contents));
            simplexml_load_string(vm, &[data_val])
        }
        Err(_) => Ok(Value::False),
    }
}

/// Stub handler for XML set handler functions that are not fully implemented
fn xml_set_stub_handler(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}
