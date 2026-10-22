//! message.c + messageobj.c + language.c: the report/dialogue messages,
//! their formatting ("line N column M - Warning: ...") and counting.
#![allow(unused_variables)]

use crate::doc::*;
use crate::enums::OptId;

/// ReleaseDate()
pub fn release_date() -> &'static str {
    "2021/07/10"
}

/// tidyLibraryVersion()
pub fn library_version() -> &'static str {
    "5.8.0"
}

/// Report(): the general report function; `args` are the va_list
/// arguments the message code expects.
pub fn report(
    doc: &mut Doc,
    element: Option<NodeId>,
    node: Option<NodeId>,
    code: u32,
    args: &[RArg],
) {
    todo!("Report")
}

/// ReportAccessError()
pub fn report_access_error(doc: &mut Doc, node: Option<NodeId>, code: u32) {
    todo!("ReportAccessError")
}

/// ReportAttrError()
pub fn report_attr_error(doc: &mut Doc, node: NodeId, av: Option<AttId>, code: u32) {
    todo!("ReportAttrError")
}

/// ReportBadArgument()
pub fn report_bad_argument(doc: &mut Doc, option: &[u8]) {
    todo!("ReportBadArgument")
}

/// ReportEntityError()
pub fn report_entity_error(doc: &mut Doc, code: u32, entity: Option<&[u8]>, c: i32) {
    todo!("ReportEntityError")
}

/// ReportFileError()
pub fn report_file_error(doc: &mut Doc, file: &[u8], code: u32) {
    todo!("ReportFileError")
}

/// ReportEncodingError()
pub fn report_encoding_error(doc: &mut Doc, code: u32, c: u32, discarded: bool) {
    todo!("ReportEncodingError")
}

/// ReportEncodingWarning()
pub fn report_encoding_warning(doc: &mut Doc, code: u32, encoding: u32) {
    todo!("ReportEncodingWarning")
}

/// ReportMissingAttr()
pub fn report_missing_attr(doc: &mut Doc, node: NodeId, name: &[u8]) {
    todo!("ReportMissingAttr")
}

/// ReportSurrogateError()
pub fn report_surrogate_error(doc: &mut Doc, code: u32, c1: u32, c2: u32) {
    todo!("ReportSurrogateError")
}

/// ReportUnknownOption()
pub fn report_unknown_option(doc: &mut Doc, option: &[u8]) {
    todo!("ReportUnknownOption")
}

/// Dialogue()
pub fn dialogue(doc: &mut Doc, code: u32, args: &[RArg]) {
    todo!("Dialogue")
}

/// ErrorSummary()
pub fn error_summary(doc: &mut Doc) {
    todo!("ErrorSummary")
}

/// ReportMarkupVersion()
pub fn report_markup_version(doc: &mut Doc) {
    todo!("ReportMarkupVersion")
}

/// ReportNumWarnings()
pub fn report_num_warnings(doc: &mut Doc) {
    todo!("ReportNumWarnings")
}

/// FreeMutedMessageList()
pub fn free_muted_message_list(doc: &mut Doc) {
    todo!("FreeMutedMessageList")
}

/// DefineMutedMessage()
pub fn define_muted_message(doc: &mut Doc, opt: OptId, name: &[u8]) {
    todo!("DefineMutedMessage")
}

/// getMutedMessageList() / getNextMutedMessage(): the keys
pub fn get_muted_messages(doc: &Doc) -> Vec<&'static str> {
    todo!("getMutedMessageList")
}

/// tidyErrorCodeAsKey()
pub fn error_code_as_key(code: u32) -> &'static str {
    todo!("tidyErrorCodeAsKey")
}

/// tidyErrorCodeFromKey()
pub fn error_code_from_key(code: &[u8]) -> u32 {
    todo!("tidyErrorCodeFromKey")
}

/// tidyOptGetDoc(): the (English) documentation of an option
pub fn opt_get_doc(opt: OptId) -> Option<&'static str> {
    todo!("tidyOptGetDoc")
}
