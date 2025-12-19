//! HTML character entity decoding

mod data;

use std::collections::HashSet;
use std::sync::LazyLock;

pub use data::NAMED_ENTITIES;

/// Legacy entities that can be used without a semicolon
pub static LEGACY_ENTITIES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "gt", "lt", "amp", "quot", "nbsp",
        "AMP", "QUOT", "GT", "LT", "COPY", "REG",
        "AElig", "Aacute", "Acirc", "Agrave", "Aring", "Atilde", "Auml",
        "Ccedil", "ETH", "Eacute", "Ecirc", "Egrave", "Euml",
        "Iacute", "Icirc", "Igrave", "Iuml", "Ntilde",
        "Oacute", "Ocirc", "Ograve", "Oslash", "Otilde", "Ouml",
        "THORN", "Uacute", "Ucirc", "Ugrave", "Uuml", "Yacute",
        "aacute", "acirc", "acute", "aelig", "agrave", "aring", "atilde", "auml",
        "brvbar", "ccedil", "cedil", "cent", "copy", "curren",
        "deg", "divide", "eacute", "ecirc", "egrave", "eth", "euml",
        "frac12", "frac14", "frac34",
        "iacute", "icirc", "iexcl", "igrave", "iquest", "iuml",
        "laquo", "macr", "micro", "middot", "not", "ntilde",
        "oacute", "ocirc", "ograve", "ordf", "ordm", "oslash", "otilde", "ouml",
        "para", "plusmn", "pound", "raquo", "reg", "sect", "shy",
        "sup1", "sup2", "sup3", "szlig", "thorn", "times",
        "uacute", "ucirc", "ugrave", "uml", "uuml", "yacute", "yen", "yuml",
    ].into_iter().collect()
});

/// HTML5 numeric character reference replacements (§13.2.5.73)
static NUMERIC_REPLACEMENTS: LazyLock<std::collections::HashMap<u32, char>> = LazyLock::new(|| {
    [
        (0x00, '\u{FFFD}'),
        (0x80, '\u{20AC}'),
        (0x82, '\u{201A}'),
        (0x83, '\u{0192}'),
        (0x84, '\u{201E}'),
        (0x85, '\u{2026}'),
        (0x86, '\u{2020}'),
        (0x87, '\u{2021}'),
        (0x88, '\u{02C6}'),
        (0x89, '\u{2030}'),
        (0x8A, '\u{0160}'),
        (0x8B, '\u{2039}'),
        (0x8C, '\u{0152}'),
        (0x8E, '\u{017D}'),
        (0x91, '\u{2018}'),
        (0x92, '\u{2019}'),
        (0x93, '\u{201C}'),
        (0x94, '\u{201D}'),
        (0x95, '\u{2022}'),
        (0x96, '\u{2013}'),
        (0x97, '\u{2014}'),
        (0x98, '\u{02DC}'),
        (0x99, '\u{2122}'),
        (0x9A, '\u{0161}'),
        (0x9B, '\u{203A}'),
        (0x9C, '\u{0153}'),
        (0x9E, '\u{017E}'),
        (0x9F, '\u{0178}'),
    ].into_iter().collect()
});

/// Decode a numeric character reference
pub fn decode_numeric_entity(text: &str, is_hex: bool) -> String {
    let codepoint = if is_hex {
        u32::from_str_radix(text, 16).ok()
    } else {
        text.parse::<u32>().ok()
    };

    let Some(codepoint) = codepoint else {
        return "\u{FFFD}".to_string();
    };

    // Check replacement table first
    if let Some(&replacement) = NUMERIC_REPLACEMENTS.get(&codepoint) {
        return replacement.to_string();
    }

    // Invalid codepoints
    if codepoint > 0x10FFFF {
        return "\u{FFFD}".to_string();
    }
    if (0xD800..=0xDFFF).contains(&codepoint) {
        return "\u{FFFD}".to_string();
    }

    // Valid codepoint
    if let Some(c) = char::from_u32(codepoint) {
        c.to_string()
    } else {
        "\u{FFFD}".to_string()
    }
}

/// Decode HTML entities in text
pub fn decode_entities_in_text(text: &str, in_attribute: bool) -> String {
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Find next &
        if chars[i] != '&' {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        let amp_pos = i;
        i += 1;

        if i >= chars.len() {
            result.push('&');
            break;
        }

        // Numeric entity
        if chars[i] == '#' {
            i += 1;
            let mut is_hex = false;

            if i < chars.len() && (chars[i] == 'x' || chars[i] == 'X') {
                is_hex = true;
                i += 1;
            }

            let digit_start = i;
            if is_hex {
                while i < chars.len() && chars[i].is_ascii_hexdigit() {
                    i += 1;
                }
            } else {
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }

            let has_semicolon = i < chars.len() && chars[i] == ';';
            let digit_text: String = chars[digit_start..i].iter().collect();

            if !digit_text.is_empty() {
                result.push_str(&decode_numeric_entity(&digit_text, is_hex));
                if has_semicolon {
                    i += 1;
                }
                continue;
            }

            // Invalid numeric entity - output as-is
            let entity: String = chars[amp_pos..if has_semicolon { i + 1 } else { i }].iter().collect();
            result.push_str(&entity);
            if has_semicolon {
                i += 1;
            }
            continue;
        }

        // Named entity
        let name_start = i;
        while i < chars.len() && chars[i].is_ascii_alphanumeric() {
            i += 1;
        }

        let entity_name: String = chars[name_start..i].iter().collect();
        let has_semicolon = i < chars.len() && chars[i] == ';';

        if entity_name.is_empty() {
            result.push('&');
            continue;
        }

        // With semicolon - exact match
        if has_semicolon {
            if let Some(decoded) = NAMED_ENTITIES.get(entity_name.as_str()) {
                result.push_str(decoded);
                i += 1; // Skip semicolon
                continue;
            }
        }

        // Legacy entity without semicolon
        if LEGACY_ENTITIES.contains(entity_name.as_str()) {
            if let Some(decoded) = NAMED_ENTITIES.get(entity_name.as_str()) {
                let next_char = if i < chars.len() { Some(chars[i]) } else { None };

                // In attributes, don't decode if followed by alphanumeric or =
                if in_attribute {
                    if let Some(next) = next_char {
                        if next.is_ascii_alphanumeric() || next == '=' {
                            result.push('&');
                            i = name_start;
                            continue;
                        }
                    }
                }

                result.push_str(decoded);
                continue;
            }
        }

        // Try prefix match for legacy entities
        let mut best_match: Option<&str> = None;
        let mut best_match_len = 0;

        for k in (1..=entity_name.len()).rev() {
            let prefix = &entity_name[..k];
            if LEGACY_ENTITIES.contains(prefix) {
                if let Some(decoded) = NAMED_ENTITIES.get(prefix) {
                    best_match = Some(decoded);
                    best_match_len = k;
                    break;
                }
            }
        }

        if let Some(matched) = best_match {
            if in_attribute {
                result.push('&');
                i = name_start;
                continue;
            }
            result.push_str(matched);
            i = name_start + best_match_len;
            continue;
        }

        // No match - keep as is
        if has_semicolon {
            let entity: String = chars[amp_pos..=i].iter().collect();
            result.push_str(&entity);
            i += 1;
        } else {
            result.push('&');
            i = name_start;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_entity() {
        assert_eq!(decode_numeric_entity("65", false), "A");
        assert_eq!(decode_numeric_entity("41", true), "A");
        assert_eq!(decode_numeric_entity("0", false), "\u{FFFD}");
    }

    #[test]
    fn test_named_entity() {
        assert_eq!(decode_entities_in_text("&amp;", false), "&");
        assert_eq!(decode_entities_in_text("&lt;", false), "<");
        assert_eq!(decode_entities_in_text("&gt;", false), ">");
    }

    #[test]
    fn test_legacy_entity() {
        assert_eq!(decode_entities_in_text("&amp", false), "&");
        assert_eq!(decode_entities_in_text("&lt", false), "<");
    }
}
