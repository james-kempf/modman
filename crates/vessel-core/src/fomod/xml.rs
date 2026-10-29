// SPDX-License-Identifier: GPL-3.0-or-later
//! Minimal element tree over quick-xml (legacy used ElementTree with
//! namespaces stripped, `fomod_py` L948-973).

use quick_xml::events::Event;
use quick_xml::Reader;

use super::FomodError;

/// One element: local tag name, attributes, direct text and children.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct El {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    /// Concatenated character data directly inside this element.
    pub text: String,
    pub children: Vec<El>,
}

impl El {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// First direct child with `tag`.
    pub fn child(&self, tag: &str) -> Option<&El> {
        self.children.iter().find(|c| c.tag == tag)
    }

    /// Direct children with `tag`.
    pub fn all(&self, tag: &str) -> Vec<&El> {
        self.children.iter().filter(|c| c.tag == tag).collect()
    }

    /// Text of the first direct child with `tag` (ElementTree `findtext`).
    pub fn child_text(&self, tag: &str) -> Option<&str> {
        self.child(tag).map(|c| c.text.as_str())
    }
}

/// Windows-1252 code points for bytes 0x80-0x9F (others map to Latin-1).
const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

fn utf16(bytes: &[u8], le: bool) -> Option<String> {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| {
            if le {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16(&units).ok()
}

/// Decodes ModuleConfig.xml bytes: UTF-16 (BOM or NUL pattern), UTF-8 (with
/// or without BOM), else Windows-1252 (legacy fallback chain L962-972).
pub(crate) fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => {
            return utf16(rest, true).unwrap_or_default();
        }
        [0xFE, 0xFF, rest @ ..] => {
            return utf16(rest, false).unwrap_or_default();
        }
        [0xEF, 0xBB, 0xBF, rest @ ..] => {
            if let Ok(s) = std::str::from_utf8(rest) {
                return s.to_string();
            }
        }
        // BOM-less UTF-16: '<' followed or preceded by NUL.
        [b'<', 0, ..] => {
            if let Some(s) = utf16(bytes, true) {
                return s;
            }
        }
        [0, b'<', ..] => {
            if let Some(s) = utf16(bytes, false) {
                return s;
            }
        }
        _ => {}
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9F => char::from_u32(u32::from(CP1252_HIGH[(b - 0x80) as usize]))
                    .unwrap_or(char::REPLACEMENT_CHARACTER),
                _ => char::from(b),
            })
            .collect(),
    }
}

fn predefined(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

fn local(name: &str) -> String {
    name.rsplit_once(':').map_or(name, |(_, l)| l).to_string()
}

fn start(e: &quick_xml::events::BytesStart<'_>) -> Result<El, FomodError> {
    let mut el = El {
        tag: local(e.local_name().as_ref()),
        ..Default::default()
    };
    for a in e.attributes().with_checks(false) {
        let a = a.map_err(|e| FomodError::Parse(e.to_string()))?;
        let v = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| FomodError::Parse(e.to_string()))?;
        el.attrs.push((local(a.key.as_ref()), v.into_owned()));
    }
    Ok(el)
}

/// Parses a document into its root element. The XML declaration is ignored:
/// the text is already decoded, and a stale `encoding="UTF-16"` must not
/// confuse the reader.
pub(crate) fn parse(text: &str) -> Result<El, FomodError> {
    let text = text.trim_start_matches('\u{feff}');
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<El> = vec![El::default()];
    let err = |e: quick_xml::Error| FomodError::Parse(e.to_string());
    loop {
        match reader.read_event().map_err(err)? {
            Event::Start(e) => stack.push(start(&e)?),
            Event::Empty(e) => {
                let el = start(&e)?;
                stack.last_mut().expect("root").children.push(el);
            }
            Event::End(_) => {
                let el = stack.pop().expect("open element");
                stack
                    .last_mut()
                    .ok_or_else(|| FomodError::Parse("unbalanced end tag".into()))?
                    .children
                    .push(el);
            }
            Event::Text(t) => stack
                .last_mut()
                .expect("root")
                .text
                .push_str(&t.xml10_content()),
            Event::CData(t) => stack
                .last_mut()
                .expect("root")
                .text
                .push_str(&t.xml10_content()),
            Event::GeneralRef(r) => {
                let c = if r.is_char_ref() {
                    r.resolve_char_ref().map_err(err)?
                } else {
                    predefined(&r.xml10_content())
                };
                let c = c.ok_or_else(|| {
                    FomodError::Parse(format!("unknown entity &{};", r.xml10_content()))
                })?;
                stack.last_mut().expect("root").text.push(c);
            }
            Event::Eof => break,
            _ => {}
        }
        if stack.is_empty() {
            return Err(FomodError::Parse("unbalanced end tag".into()));
        }
    }
    if stack.len() != 1 {
        return Err(FomodError::Parse("unexpected end of document".into()));
    }
    stack
        .pop()
        .and_then(|doc| doc.children.into_iter().next())
        .ok_or_else(|| FomodError::Parse("no root element".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_namespaces_and_resolves_entities() {
        let el = parse(
            r#"<?xml version="1.0" encoding="UTF-16"?>
            <xs:config xmlns:xs="u" xs:a="x &amp; y"><b>T&#233;st &lt;1&gt;</b><c/></xs:config>"#,
        )
        .unwrap();
        assert_eq!(el.tag, "config");
        assert_eq!(el.attr("a"), Some("x & y"));
        assert_eq!(el.child_text("b"), Some("Tést <1>"));
        assert!(el.child("c").is_some());
    }

    #[test]
    fn decodes_utf16_and_cp1252() {
        let src = "<config><n>Ünïcode “q”</n></config>";
        let mut le = vec![0xFF, 0xFE];
        le.extend(src.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode(&le), src);
        let be: Vec<u8> = src.encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(decode(&be), src);
        // cp1252: 0x93/0x94 are curly quotes, 0xDC is Ü.
        let cp = b"<n>\xDC \x93q\x94</n>";
        assert_eq!(decode(cp), "<n>Ü “q”</n>");
    }

    #[test]
    fn malformed_xml_is_a_parse_error() {
        assert!(matches!(parse("<a><b></a>"), Err(FomodError::Parse(_))));
        assert!(matches!(parse(""), Err(FomodError::Parse(_))));
    }
}
