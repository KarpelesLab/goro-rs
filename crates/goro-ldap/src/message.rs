//! Parsed LDAP messages (what an LDAPMessage* holds for the PHP layer).

/// A control as parsed from the wire or built from a PHP array.
#[derive(Debug, Clone)]
pub struct Control {
    pub oid: Vec<u8>,
    pub criticality: bool,
    pub value: Option<Vec<u8>>,
}

/// A search result entry: DN and ordered attributes.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub dn: Vec<u8>,
    /// (attribute name, values) in wire order.
    pub attrs: Vec<(Vec<u8>, Vec<Vec<u8>>)>,
}

/// A search result reference.
#[derive(Debug, Clone, Default)]
pub struct Reference {
    pub referrals: Vec<Vec<u8>>,
}

/// Everything an ldap_search / operation result exposes (an LDAPMessage).
#[derive(Debug, Clone, Default)]
pub struct LdapMessage {
    pub entries: Vec<Entry>,
    pub references: Vec<Reference>,
    /// result code of the operation's final response
    pub rescode: i32,
    pub matched_dn: Vec<u8>,
    pub errmsg: Vec<u8>,
    /// referrals carried in the result message
    pub referrals: Vec<Vec<u8>>,
    /// response controls on the final result message
    pub ctrls: Vec<Control>,
    /// extended-response fields (ldap_parse_exop / exop sync)
    pub exop_oid: Option<Vec<u8>>,
    pub exop_data: Option<Vec<u8>>,
    /// SASL server credentials (bind response)
    pub sasl_creds: Option<Vec<u8>>,
}
