//! Serialización del XML `java.util.Properties` del esquema heredado en
//! `ticketlines.ATTRIBUTES` y `receipts.ATTRIBUTES`.

use std::collections::BTreeMap;

const HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
<!DOCTYPE properties SYSTEM \"http://java.sun.com/dtd/properties.dtd\">\n\
<properties>\n\
<comment>POSrust</comment>\n";

/// Genera el XML compatible con `Properties.storeToXML`.
pub fn properties_xml(entries: &BTreeMap<String, String>) -> Vec<u8> {
    let mut out = String::from(HEADER);
    for (key, value) in entries {
        out.push_str("<entry key=\"");
        escape_into(&mut out, key);
        out.push_str("\">");
        escape_into(&mut out, value);
        out.push_str("</entry>\n");
    }
    out.push_str("</properties>\n");
    out.into_bytes()
}

fn escape_into(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            _ if !c.is_control() && !matches!(c, '\u{FFFE}' | '\u{FFFF}') => out.push(c),
            _ => out.push('\u{FFFD}'),
        }
    }
}

/// Lee Properties sin resolver DTDs ni entidades externas. Un BLOB inválido
/// queda sin atributos en la vista, nunca se modifica en la base.
pub(crate) fn read_properties(bytes: &[u8]) -> BTreeMap<String, String> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true, // Properties de Java incluye un DOCTYPE.
        nodes_limit: 8_192,
    };
    let Some(doc) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|xml| roxmltree::Document::parse_with_options(xml, options).ok())
    else {
        return BTreeMap::new();
    };
    let root = doc.root_element();
    if !root.has_tag_name("properties") {
        return BTreeMap::new();
    }
    root.children()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|node| Some((
            node.attribute("key")?.to_owned(),
            node.children().filter_map(|child| child.text()).collect(),
        )))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genera_xml_con_escapado() {
        let mut map = BTreeMap::new();
        map.insert("product.name".to_string(), "Café & <leche>".to_string());
        let xml = String::from_utf8(properties_xml(&map)).unwrap();
        assert!(xml.contains("<entry key=\"product.name\">Café &amp; &lt;leche&gt;</entry>"));
        assert!(xml.ends_with("</properties>\n"));
        assert_eq!(read_properties(xml.as_bytes()), map);
    }
}
