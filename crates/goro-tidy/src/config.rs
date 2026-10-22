//! config.c: the configuration options (names, types, defaults, pick
//! lists, parsers), configuration files and option values.
#![allow(unused_variables)]

use crate::doc::*;
use crate::enums::{N_TIDY_OPTIONS, OptId};
use crate::streamio::StreamIn;

/// TidyOptionType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionType {
    String,
    Integer,
    Boolean,
}

/// TidyConfigCategory
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfigCategory {
    Diagnostics = 301,
    Display,
    DocumentIO,
    Encoding,
    FileIO,
    MarkupCleanup,
    MarkupEntities,
    MarkupRepair,
    MarkupTeach,
    MarkupXForm,
    PrettyPrint,
    InternalCategory,
}

/// `ParseProperty`
pub type ParsePropertyFn = fn(&mut Doc, &'static OptionImpl) -> bool;

/// PickListItem
#[derive(Debug)]
pub struct PickListItem {
    pub label: &'static str,
    pub value: i32,
    pub inputs: &'static [&'static str],
}

/// _tidy_option (TidyOptionImpl)
#[derive(Debug)]
pub struct OptionImpl {
    pub id: OptId,
    pub category: ConfigCategory,
    pub name: &'static str,
    pub type_: OptionType,
    /// Default value for TidyInteger and TidyBoolean
    pub dflt: u64,
    /// Function to parse input; read-only if None.
    pub parser: Option<ParsePropertyFn>,
    pub pick_list: Option<&'static [PickListItem]>,
    /// Default value for TidyString.
    pub pdflt: Option<&'static str>,
}

/// TidyConfigImpl
#[derive(Clone, Debug)]
pub struct ConfigImpl {
    pub value: Vec<OptValue>,
    pub snapshot: Vec<OptValue>,
    pub defined_tags: u32,
    /// Current char in input stream for reading options.
    pub c: u32,
    /// Current input source for reading options.
    pub cfg_in: Option<StreamIn>,
}

impl Default for ConfigImpl {
    fn default() -> Self {
        ConfigImpl {
            value: vec![OptValue::default(); N_TIDY_OPTIONS + 1],
            snapshot: vec![OptValue::default(); N_TIDY_OPTIONS + 1],
            defined_tags: 0,
            c: 0,
            cfg_in: None,
        }
    }
}

/// lookupOption()
pub fn lookup_option(optnam: &[u8]) -> Option<&'static OptionImpl> {
    todo!("lookupOption")
}

/// getOption()
pub fn get_option(opt_id: OptId) -> &'static OptionImpl {
    todo!("getOption")
}

/// getOptionIsList()
pub fn get_option_is_list(opt_id: OptId) -> bool {
    todo!("getOptionIsList")
}

/// getOptionList() / getNextOption(): the options tidyGetNextOption()
/// iterates over, in order.
pub fn get_option_list(doc: &Doc) -> Vec<&'static OptionImpl> {
    todo!("getOptionList")
}

/// getOptionPickList() / getNextOptionPick(): the pick labels
pub fn get_option_pick_list(option: &'static OptionImpl) -> Vec<&'static str> {
    todo!("getOptionPickList")
}

/// InitConfig()
pub fn init_config(doc: &mut Doc) {
    todo!("InitConfig")
}

/// FreeConfig()
pub fn free_config(doc: &mut Doc) {
    todo!("FreeConfig")
}

/// GetPickListLabelForPick()
pub fn get_pick_list_label_for_pick(opt_id: OptId, pick: u32) -> Option<&'static str> {
    todo!("GetPickListLabelForPick")
}

/// SetOptionInt()
pub fn set_option_int(doc: &mut Doc, opt_id: OptId, val: u64) -> bool {
    todo!("SetOptionInt")
}

/// SetOptionBool()
pub fn set_option_bool(doc: &mut Doc, opt_id: OptId, val: bool) -> bool {
    todo!("SetOptionBool")
}

/// ResetOptionToDefault()
pub fn reset_option_to_default(doc: &mut Doc, opt_id: OptId) -> bool {
    todo!("ResetOptionToDefault")
}

/// ResetConfigToDefault()
pub fn reset_config_to_default(doc: &mut Doc) {
    todo!("ResetConfigToDefault")
}

/// TakeConfigSnapshot()
pub fn take_config_snapshot(doc: &mut Doc) {
    todo!("TakeConfigSnapshot")
}

/// ResetConfigToSnapshot()
pub fn reset_config_to_snapshot(doc: &mut Doc) {
    todo!("ResetConfigToSnapshot")
}

/// ParseConfigFile()
pub fn parse_config_file(doc: &mut Doc, cfgfil: &[u8]) -> i32 {
    todo!("ParseConfigFile")
}

/// ParseConfigFileEnc()
pub fn parse_config_file_enc(doc: &mut Doc, cfgfil: &[u8], charenc: &[u8]) -> i32 {
    todo!("ParseConfigFileEnc")
}

/// ParseConfigOption(): option name and value
pub fn parse_config_option(doc: &mut Doc, optnam: &[u8], opt_val: &[u8]) -> bool {
    todo!("ParseConfigOption")
}

/// ParseConfigValue(): option id and value
pub fn parse_config_value(doc: &mut Doc, opt_id: OptId, opt_val: &[u8]) -> bool {
    todo!("ParseConfigValue")
}

/// AdjustCharEncoding()
pub fn adjust_char_encoding(doc: &mut Doc, encoding: i32) -> bool {
    todo!("AdjustCharEncoding")
}

/// AdjustConfig(): ensure that config is self consistent
pub fn adjust_config(doc: &mut Doc) {
    todo!("AdjustConfig")
}

/// ConfigDiffThanDefault()
pub fn config_diff_than_default(doc: &mut Doc) -> bool {
    todo!("ConfigDiffThanDefault")
}

/// ConfigDiffThanSnapshot()
pub fn config_diff_than_snapshot(doc: &mut Doc) -> bool {
    todo!("ConfigDiffThanSnapshot")
}

/// CharEncodingId()
pub fn char_encoding_id(doc: &mut Doc, charenc: &[u8]) -> i32 {
    todo!("CharEncodingId")
}

/// CharEncodingName()
pub fn char_encoding_name(encoding: i32) -> &'static str {
    todo!("CharEncodingName")
}

/// CharEncodingOptName()
pub fn char_encoding_opt_name(encoding: i32) -> &'static str {
    todo!("CharEncodingOptName")
}

/// DeclareListItem()
pub fn declare_list_item(doc: &mut Doc, opt: &'static OptionImpl, name: &[u8]) {
    todo!("DeclareListItem")
}
