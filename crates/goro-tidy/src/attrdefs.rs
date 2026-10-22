//! Generated from tidy-html5 5.8.0 src/attrs.c attribute_defs[] (gen_tables.py).
use crate::attrs::*;
use crate::enums::AttrId;

pub static ATTRIBUTE_DEFS: &[Attribute] = &[
    Attribute {
        id: AttrId::UNKNOWN,
        name: b"unknown!",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ABBR,
        name: b"abbr",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ACCEPT,
        name: b"accept",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ACCEPT_CHARSET,
        name: b"accept-charset",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ACCESSKEY,
        name: b"accesskey",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ACTION,
        name: b"action",
        attrchk: Some(check_action as AttrCheckFn),
    },
    Attribute {
        id: AttrId::ADD_DATE,
        name: b"add_date",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ALIGN,
        name: b"align",
        attrchk: Some(check_align as AttrCheckFn),
    },
    Attribute {
        id: AttrId::ALINK,
        name: b"alink",
        attrchk: Some(check_color as AttrCheckFn),
    },
    Attribute {
        id: AttrId::ALLOWFULLSCREEN,
        name: b"allowfullscreen",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::ALT,
        name: b"alt",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARCHIVE,
        name: b"archive",
        attrchk: None,
    },
    Attribute {
        id: AttrId::AXIS,
        name: b"axis",
        attrchk: None,
    },
    Attribute {
        id: AttrId::BACKGROUND,
        name: b"background",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::BGCOLOR,
        name: b"bgcolor",
        attrchk: Some(check_color as AttrCheckFn),
    },
    Attribute {
        id: AttrId::BGPROPERTIES,
        name: b"bgproperties",
        attrchk: None,
    },
    Attribute {
        id: AttrId::BORDER,
        name: b"border",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::BORDERCOLOR,
        name: b"bordercolor",
        attrchk: Some(check_color as AttrCheckFn),
    },
    Attribute {
        id: AttrId::BOTTOMMARGIN,
        name: b"bottommargin",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CELLPADDING,
        name: b"cellpadding",
        attrchk: Some(check_length as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CELLSPACING,
        name: b"cellspacing",
        attrchk: Some(check_length as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CHAR,
        name: b"char",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CHAROFF,
        name: b"charoff",
        attrchk: Some(check_length as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CHARSET,
        name: b"charset",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CHECKED,
        name: b"checked",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CITE,
        name: b"cite",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CLASS,
        name: b"class",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CLASSID,
        name: b"classid",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CLEAR,
        name: b"clear",
        attrchk: Some(check_clear as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CODE,
        name: b"code",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CODEBASE,
        name: b"codebase",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CODETYPE,
        name: b"codetype",
        attrchk: None,
    },
    Attribute {
        id: AttrId::COLOR,
        name: b"color",
        attrchk: Some(check_color as AttrCheckFn),
    },
    Attribute {
        id: AttrId::COLS,
        name: b"cols",
        attrchk: None,
    },
    Attribute {
        id: AttrId::COLSPAN,
        name: b"colspan",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::COMPACT,
        name: b"compact",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::CONTENT,
        name: b"content",
        attrchk: None,
    },
    Attribute {
        id: AttrId::COORDS,
        name: b"coords",
        attrchk: None,
    },
    Attribute {
        id: AttrId::DATA,
        name: b"data",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::DATAFLD,
        name: b"datafld",
        attrchk: None,
    },
    Attribute {
        id: AttrId::DATAFORMATAS,
        name: b"dataformatas",
        attrchk: None,
    },
    Attribute {
        id: AttrId::DATAPAGESIZE,
        name: b"datapagesize",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::DATASRC,
        name: b"datasrc",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::DATETIME,
        name: b"datetime",
        attrchk: None,
    },
    Attribute {
        id: AttrId::DECLARE,
        name: b"declare",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::DEFER,
        name: b"defer",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::DIR,
        name: b"dir",
        attrchk: Some(check_text_dir as AttrCheckFn),
    },
    Attribute {
        id: AttrId::DISABLED,
        name: b"disabled",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::DOWNLOAD,
        name: b"download",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ENCODING,
        name: b"encoding",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ENCTYPE,
        name: b"enctype",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FACE,
        name: b"face",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FOR,
        name: b"for",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FRAME,
        name: b"frame",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FRAMEBORDER,
        name: b"frameborder",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FRAMESPACING,
        name: b"framespacing",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::GRIDX,
        name: b"gridx",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::GRIDY,
        name: b"gridy",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::HEADERS,
        name: b"headers",
        attrchk: None,
    },
    Attribute {
        id: AttrId::HEIGHT,
        name: b"height",
        attrchk: Some(check_length as AttrCheckFn),
    },
    Attribute {
        id: AttrId::HREF,
        name: b"href",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::HREFLANG,
        name: b"hreflang",
        attrchk: Some(check_lang as AttrCheckFn),
    },
    Attribute {
        id: AttrId::HSPACE,
        name: b"hspace",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::HTTP_EQUIV,
        name: b"http-equiv",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ID,
        name: b"id",
        attrchk: Some(check_id as AttrCheckFn),
    },
    Attribute {
        id: AttrId::IS,
        name: b"is",
        attrchk: Some(check_is as AttrCheckFn),
    },
    Attribute {
        id: AttrId::ISMAP,
        name: b"ismap",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::ITEMID,
        name: b"itemid",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ITEMPROP,
        name: b"itemprop",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ITEMREF,
        name: b"itemref",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ITEMSCOPE,
        name: b"itemscope",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::ITEMTYPE,
        name: b"itemtype",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::LABEL,
        name: b"label",
        attrchk: None,
    },
    Attribute {
        id: AttrId::LANG,
        name: b"lang",
        attrchk: Some(check_lang as AttrCheckFn),
    },
    Attribute {
        id: AttrId::LANGUAGE,
        name: b"language",
        attrchk: None,
    },
    Attribute {
        id: AttrId::LAST_MODIFIED,
        name: b"last_modified",
        attrchk: None,
    },
    Attribute {
        id: AttrId::LAST_VISIT,
        name: b"last_visit",
        attrchk: None,
    },
    Attribute {
        id: AttrId::LEFTMARGIN,
        name: b"leftmargin",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::LINK,
        name: b"link",
        attrchk: Some(check_color as AttrCheckFn),
    },
    Attribute {
        id: AttrId::LONGDESC,
        name: b"longdesc",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::LOWSRC,
        name: b"lowsrc",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::MARGINHEIGHT,
        name: b"marginheight",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::MARGINWIDTH,
        name: b"marginwidth",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::MAXLENGTH,
        name: b"maxlength",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::MEDIA,
        name: b"media",
        attrchk: None,
    },
    Attribute {
        id: AttrId::METHOD,
        name: b"method",
        attrchk: Some(check_fsubmit as AttrCheckFn),
    },
    Attribute {
        id: AttrId::MULTIPLE,
        name: b"multiple",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::NAME,
        name: b"name",
        attrchk: Some(check_name as AttrCheckFn),
    },
    Attribute {
        id: AttrId::NOHREF,
        name: b"nohref",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::NORESIZE,
        name: b"noresize",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::NOSHADE,
        name: b"noshade",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::NOWRAP,
        name: b"nowrap",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OBJECT,
        name: b"object",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnAFTERUPDATE,
        name: b"onafterupdate",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnBEFOREUNLOAD,
        name: b"onbeforeunload",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnBEFOREUPDATE,
        name: b"onbeforeupdate",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnBLUR,
        name: b"onblur",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnCHANGE,
        name: b"onchange",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnCLICK,
        name: b"onclick",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnDATAAVAILABLE,
        name: b"ondataavailable",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnDATASETCHANGED,
        name: b"ondatasetchanged",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnDATASETCOMPLETE,
        name: b"ondatasetcomplete",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnDBLCLICK,
        name: b"ondblclick",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnERRORUPDATE,
        name: b"onerrorupdate",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnFOCUS,
        name: b"onfocus",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnKEYDOWN,
        name: b"onkeydown",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnKEYPRESS,
        name: b"onkeypress",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnKEYUP,
        name: b"onkeyup",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnLOAD,
        name: b"onload",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnMOUSEDOWN,
        name: b"onmousedown",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnMOUSEMOVE,
        name: b"onmousemove",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnMOUSEOUT,
        name: b"onmouseout",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnMOUSEOVER,
        name: b"onmouseover",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnMOUSEUP,
        name: b"onmouseup",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnRESET,
        name: b"onreset",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnROWENTER,
        name: b"onrowenter",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnROWEXIT,
        name: b"onrowexit",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnSELECT,
        name: b"onselect",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnSUBMIT,
        name: b"onsubmit",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OnUNLOAD,
        name: b"onunload",
        attrchk: Some(check_script as AttrCheckFn),
    },
    Attribute {
        id: AttrId::PROFILE,
        name: b"profile",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::PROMPT,
        name: b"prompt",
        attrchk: None,
    },
    Attribute {
        id: AttrId::RBSPAN,
        name: b"rbspan",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::READONLY,
        name: b"readonly",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::REL,
        name: b"rel",
        attrchk: None,
    },
    Attribute {
        id: AttrId::REV,
        name: b"rev",
        attrchk: None,
    },
    Attribute {
        id: AttrId::RIGHTMARGIN,
        name: b"rightmargin",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::ROLE,
        name: b"role",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ROWS,
        name: b"rows",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::ROWSPAN,
        name: b"rowspan",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::RULES,
        name: b"rules",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SCHEME,
        name: b"scheme",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SCOPE,
        name: b"scope",
        attrchk: Some(check_scope as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SCROLLING,
        name: b"scrolling",
        attrchk: Some(check_scroll as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SELECTED,
        name: b"selected",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SHAPE,
        name: b"shape",
        attrchk: Some(check_shape as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SHOWGRID,
        name: b"showgrid",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SHOWGRIDX,
        name: b"showgridx",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SHOWGRIDY,
        name: b"showgridy",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SIZE,
        name: b"size",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SPAN,
        name: b"span",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SRC,
        name: b"src",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SRCSET,
        name: b"srcset",
        attrchk: None,
    },
    Attribute {
        id: AttrId::STANDBY,
        name: b"standby",
        attrchk: None,
    },
    Attribute {
        id: AttrId::START,
        name: b"start",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::STYLE,
        name: b"style",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SUMMARY,
        name: b"summary",
        attrchk: None,
    },
    Attribute {
        id: AttrId::TABINDEX,
        name: b"tabindex",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::TARGET,
        name: b"target",
        attrchk: Some(check_target as AttrCheckFn),
    },
    Attribute {
        id: AttrId::TEXT,
        name: b"text",
        attrchk: Some(check_color as AttrCheckFn),
    },
    Attribute {
        id: AttrId::TITLE,
        name: b"title",
        attrchk: None,
    },
    Attribute {
        id: AttrId::TOPMARGIN,
        name: b"topmargin",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::TRANSLATE,
        name: b"translate",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::TYPE,
        name: b"type",
        attrchk: Some(check_type as AttrCheckFn),
    },
    Attribute {
        id: AttrId::USEMAP,
        name: b"usemap",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::VALIGN,
        name: b"valign",
        attrchk: Some(check_valign as AttrCheckFn),
    },
    Attribute {
        id: AttrId::VALUE,
        name: b"value",
        attrchk: None,
    },
    Attribute {
        id: AttrId::VALUETYPE,
        name: b"valuetype",
        attrchk: Some(check_v_type as AttrCheckFn),
    },
    Attribute {
        id: AttrId::VERSION,
        name: b"version",
        attrchk: None,
    },
    Attribute {
        id: AttrId::VLINK,
        name: b"vlink",
        attrchk: Some(check_color as AttrCheckFn),
    },
    Attribute {
        id: AttrId::VSPACE,
        name: b"vspace",
        attrchk: Some(check_number as AttrCheckFn),
    },
    Attribute {
        id: AttrId::WIDTH,
        name: b"width",
        attrchk: Some(check_length as AttrCheckFn),
    },
    Attribute {
        id: AttrId::WRAP,
        name: b"wrap",
        attrchk: None,
    },
    Attribute {
        id: AttrId::XML_LANG,
        name: b"xml:lang",
        attrchk: Some(check_lang as AttrCheckFn),
    },
    Attribute {
        id: AttrId::XML_SPACE,
        name: b"xml:space",
        attrchk: None,
    },
    Attribute {
        id: AttrId::XMLNS,
        name: b"xmlns",
        attrchk: None,
    },
    Attribute {
        id: AttrId::EVENT,
        name: b"event",
        attrchk: None,
    },
    Attribute {
        id: AttrId::METHODS,
        name: b"methods",
        attrchk: None,
    },
    Attribute {
        id: AttrId::N,
        name: b"n",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SDAFORM,
        name: b"sdaform",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SDAPREF,
        name: b"sdapref",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SDASUFF,
        name: b"sdasuff",
        attrchk: None,
    },
    Attribute {
        id: AttrId::URN,
        name: b"urn",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ASYNC,
        name: b"async",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::AUTOCOMPLETE,
        name: b"autocomplete",
        attrchk: None,
    },
    Attribute {
        id: AttrId::AUTOFOCUS,
        name: b"autofocus",
        attrchk: None,
    },
    Attribute {
        id: AttrId::AUTOPLAY,
        name: b"autoplay",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CHALLENGE,
        name: b"challenge",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CONTENTEDITABLE,
        name: b"contenteditable",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CONTEXTMENU,
        name: b"contextmenu",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CONTROLS,
        name: b"controls",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CROSSORIGIN,
        name: b"crossorigin",
        attrchk: None,
    },
    Attribute {
        id: AttrId::DEFAULT,
        name: b"default",
        attrchk: None,
    },
    Attribute {
        id: AttrId::DIRNAME,
        name: b"dirname",
        attrchk: None,
    },
    Attribute {
        id: AttrId::DRAGGABLE,
        name: b"draggable",
        attrchk: None,
    },
    Attribute {
        id: AttrId::DROPZONE,
        name: b"dropzone",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FORM,
        name: b"form",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FORMACTION,
        name: b"formaction",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FORMENCTYPE,
        name: b"formenctype",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FORMMETHOD,
        name: b"formmethod",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FORMNOVALIDATE,
        name: b"formnovalidate",
        attrchk: None,
    },
    Attribute {
        id: AttrId::FORMTARGET,
        name: b"formtarget",
        attrchk: None,
    },
    Attribute {
        id: AttrId::HIDDEN,
        name: b"hidden",
        attrchk: None,
    },
    Attribute {
        id: AttrId::HIGH,
        name: b"high",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ICON,
        name: b"icon",
        attrchk: None,
    },
    Attribute {
        id: AttrId::KEYTYPE,
        name: b"keytype",
        attrchk: None,
    },
    Attribute {
        id: AttrId::KIND,
        name: b"kind",
        attrchk: None,
    },
    Attribute {
        id: AttrId::LIST,
        name: b"list",
        attrchk: None,
    },
    Attribute {
        id: AttrId::LOOP,
        name: b"loop",
        attrchk: None,
    },
    Attribute {
        id: AttrId::LOW,
        name: b"low",
        attrchk: None,
    },
    Attribute {
        id: AttrId::MANIFEST,
        name: b"manifest",
        attrchk: None,
    },
    Attribute {
        id: AttrId::MAX,
        name: b"max",
        attrchk: None,
    },
    Attribute {
        id: AttrId::MEDIAGROUP,
        name: b"mediagroup",
        attrchk: None,
    },
    Attribute {
        id: AttrId::MIN,
        name: b"min",
        attrchk: None,
    },
    Attribute {
        id: AttrId::MUTED,
        name: b"muted",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::NOVALIDATE,
        name: b"novalidate",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OPEN,
        name: b"open",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OPTIMUM,
        name: b"optimum",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnABORT,
        name: b"onabort",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnAFTERPRINT,
        name: b"onafterprint",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnBEFOREPRINT,
        name: b"onbeforeprint",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnCANPLAY,
        name: b"oncanplay",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnCANPLAYTHROUGH,
        name: b"oncanplaythrough",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnCONTEXTMENU,
        name: b"oncontextmenu",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnCUECHANGE,
        name: b"oncuechange",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnDRAG,
        name: b"ondrag",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnDRAGEND,
        name: b"ondragend",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnDRAGENTER,
        name: b"ondragenter",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnDRAGLEAVE,
        name: b"ondragleave",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnDRAGOVER,
        name: b"ondragover",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnDRAGSTART,
        name: b"ondragstart",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnDROP,
        name: b"ondrop",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnDURATIONCHANGE,
        name: b"ondurationchange",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnEMPTIED,
        name: b"onemptied",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnENDED,
        name: b"onended",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnERROR,
        name: b"onerror",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnHASHCHANGE,
        name: b"onhashchange",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnINPUT,
        name: b"oninput",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnINVALID,
        name: b"oninvalid",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnLOADEDDATA,
        name: b"onloadeddata",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnLOADEDMETADATA,
        name: b"onloadedmetadata",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnLOADSTART,
        name: b"onloadstart",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnMESSAGE,
        name: b"onmessage",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnMOUSEWHEEL,
        name: b"onmousewheel",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnOFFLINE,
        name: b"onoffline",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnONLINE,
        name: b"ononline",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnPAGEHIDE,
        name: b"onpagehide",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnPAGESHOW,
        name: b"onpageshow",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnPAUSE,
        name: b"onpause",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnPLAY,
        name: b"onplay",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnPLAYING,
        name: b"onplaying",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnPOPSTATE,
        name: b"onpopstate",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnPROGRESS,
        name: b"onprogress",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnRATECHANGE,
        name: b"onratechange",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnREADYSTATECHANGE,
        name: b"onreadystatechange",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnREDO,
        name: b"onredo",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnRESIZE,
        name: b"onresize",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnSCROLL,
        name: b"onscroll",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnSEEKED,
        name: b"onseeked",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnSEEKING,
        name: b"onseeking",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnSHOW,
        name: b"onshow",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnSTALLED,
        name: b"onstalled",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnSTORAGE,
        name: b"onstorage",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnSUSPEND,
        name: b"onsuspend",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnTIMEUPDATE,
        name: b"ontimeupdate",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnUNDO,
        name: b"onundo",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnVOLUMECHANGE,
        name: b"onvolumechange",
        attrchk: None,
    },
    Attribute {
        id: AttrId::OnWAITING,
        name: b"onwaiting",
        attrchk: None,
    },
    Attribute {
        id: AttrId::PATTERN,
        name: b"pattern",
        attrchk: None,
    },
    Attribute {
        id: AttrId::PLACEHOLDER,
        name: b"placeholder",
        attrchk: None,
    },
    Attribute {
        id: AttrId::PLAYSINLINE,
        name: b"playsinline",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::POSTER,
        name: b"poster",
        attrchk: None,
    },
    Attribute {
        id: AttrId::PRELOAD,
        name: b"preload",
        attrchk: None,
    },
    Attribute {
        id: AttrId::PUBDATE,
        name: b"pubdate",
        attrchk: None,
    },
    Attribute {
        id: AttrId::RADIOGROUP,
        name: b"radiogroup",
        attrchk: None,
    },
    Attribute {
        id: AttrId::REQUIRED,
        name: b"required",
        attrchk: None,
    },
    Attribute {
        id: AttrId::REVERSED,
        name: b"reversed",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SANDBOX,
        name: b"sandbox",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SCOPED,
        name: b"scoped",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SEAMLESS,
        name: b"seamless",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SIZES,
        name: b"sizes",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SPELLCHECK,
        name: b"spellcheck",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SRCDOC,
        name: b"srcdoc",
        attrchk: None,
    },
    Attribute {
        id: AttrId::SRCLANG,
        name: b"srclang",
        attrchk: None,
    },
    Attribute {
        id: AttrId::STEP,
        name: b"step",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_ACTIVEDESCENDANT,
        name: b"aria-activedescendant",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_ATOMIC,
        name: b"aria-atomic",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_AUTOCOMPLETE,
        name: b"aria-autocomplete",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_BUSY,
        name: b"aria-busy",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_CHECKED,
        name: b"aria-checked",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_CONTROLS,
        name: b"aria-controls",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_DESCRIBEDBY,
        name: b"aria-describedby",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_DISABLED,
        name: b"aria-disabled",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_DROPEFFECT,
        name: b"aria-dropeffect",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_EXPANDED,
        name: b"aria-expanded",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_FLOWTO,
        name: b"aria-flowto",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_GRABBED,
        name: b"aria-grabbed",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_HASPOPUP,
        name: b"aria-haspopup",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_HIDDEN,
        name: b"aria-hidden",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_INVALID,
        name: b"aria-invalid",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_LABEL,
        name: b"aria-label",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_LABELLEDBY,
        name: b"aria-labelledby",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_LEVEL,
        name: b"aria-level",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_LIVE,
        name: b"aria-live",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_MULTILINE,
        name: b"aria-multiline",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_MULTISELECTABLE,
        name: b"aria-multiselectable",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_ORIENTATION,
        name: b"aria-orientation",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_OWNS,
        name: b"aria-owns",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_POSINSET,
        name: b"aria-posinset",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_PRESSED,
        name: b"aria-pressed",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_READONLY,
        name: b"aria-readonly",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_RELEVANT,
        name: b"aria-relevant",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_REQUIRED,
        name: b"aria-required",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_SELECTED,
        name: b"aria-selected",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_SETSIZE,
        name: b"aria-setsize",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_SORT,
        name: b"aria-sort",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_VALUEMAX,
        name: b"aria-valuemax",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_VALUEMIN,
        name: b"aria-valuemin",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_VALUENOW,
        name: b"aria-valuenow",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ARIA_VALUETEXT,
        name: b"aria-valuetext",
        attrchk: None,
    },
    Attribute {
        id: AttrId::X,
        name: b"x",
        attrchk: None,
    },
    Attribute {
        id: AttrId::Y,
        name: b"y",
        attrchk: None,
    },
    Attribute {
        id: AttrId::VIEWBOX,
        name: b"viewbox",
        attrchk: None,
    },
    Attribute {
        id: AttrId::PRESERVEASPECTRATIO,
        name: b"preserveaspectratio",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ZOOMANDPAN,
        name: b"zoomandpan",
        attrchk: None,
    },
    Attribute {
        id: AttrId::BASEPROFILE,
        name: b"baseprofile",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CONTENTSCRIPTTYPE,
        name: b"contentscripttype",
        attrchk: None,
    },
    Attribute {
        id: AttrId::CONTENTSTYLETYPE,
        name: b"contentstyletype",
        attrchk: None,
    },
    Attribute {
        id: AttrId::DISPLAY,
        name: b"display",
        attrchk: None,
    },
    Attribute {
        id: AttrId::ABOUT,
        name: b"about",
        attrchk: Some(check_rd_fa_safe_curie as AttrCheckFn),
    },
    Attribute {
        id: AttrId::DATATYPE,
        name: b"datatype",
        attrchk: Some(check_rd_fa_term as AttrCheckFn),
    },
    Attribute {
        id: AttrId::INLIST,
        name: b"inlist",
        attrchk: Some(check_bool as AttrCheckFn),
    },
    Attribute {
        id: AttrId::PREFIX,
        name: b"prefix",
        attrchk: Some(check_rd_fa_prefix as AttrCheckFn),
    },
    Attribute {
        id: AttrId::PROPERTY,
        name: b"property",
        attrchk: Some(check_rd_fa_term as AttrCheckFn),
    },
    Attribute {
        id: AttrId::RESOURCE,
        name: b"resource",
        attrchk: Some(check_rd_fa_safe_curie as AttrCheckFn),
    },
    Attribute {
        id: AttrId::TYPEOF,
        name: b"typeof",
        attrchk: Some(check_rd_fa_term as AttrCheckFn),
    },
    Attribute {
        id: AttrId::VOCAB,
        name: b"vocab",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::INTEGRITY,
        name: b"integrity",
        attrchk: None,
    },
    Attribute {
        id: AttrId::AS,
        name: b"as",
        attrchk: None,
    },
    Attribute {
        id: AttrId::XMLNSXLINK,
        name: b"xmlns:xlink",
        attrchk: Some(check_url as AttrCheckFn),
    },
    Attribute {
        id: AttrId::SLOT,
        name: b"slot",
        attrchk: None,
    },
    Attribute {
        id: AttrId::LOADING,
        name: b"loading",
        attrchk: Some(check_loading as AttrCheckFn),
    },
    Attribute {
        id: AttrId::FILL,
        name: b"fill",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::FILLRULE,
        name: b"fill-rule",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::STROKE,
        name: b"stroke",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::STROKEDASHARRAY,
        name: b"stroke-dasharray",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::STROKEDASHOFFSET,
        name: b"stroke-dashoffset",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::STROKELINECAP,
        name: b"stroke-linecap",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::STROKELINEJOIN,
        name: b"stroke-linejoin",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::STROKEMITERLIMIT,
        name: b"stroke-miterlimit",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::STROKEWIDTH,
        name: b"stroke-width",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::COLORINTERPOLATION,
        name: b"color-interpolation",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::COLORRENDERING,
        name: b"color-rendering",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::OPACITY,
        name: b"opacity",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::STROKEOPACITY,
        name: b"stroke-opacity",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
    Attribute {
        id: AttrId::FILLOPACITY,
        name: b"fill-opacity",
        attrchk: Some(check_svg_attr as AttrCheckFn),
    },
];
