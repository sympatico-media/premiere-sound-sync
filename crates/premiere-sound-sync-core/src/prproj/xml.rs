//! Minimal XML tag scanner. Enough to locate elements by byte offset without re-serializing.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Open,
    Close,
    Empty,
}

#[derive(Debug, Clone, Copy)]
pub struct Tag<'a> {
    pub kind: Kind,
    pub name: &'a str,
    pub attrs: &'a str,
    /// Byte offset of `<`.
    pub start: usize,
    /// Byte offset just past `>`.
    pub end: usize,
}

pub struct Tags<'a> {
    xml: &'a str,
    pos: usize,
}

pub fn tags(xml: &str) -> Tags<'_> {
    Tags { xml, pos: 0 }
}

impl<'a> Iterator for Tags<'a> {
    type Item = Tag<'a>;

    fn next(&mut self) -> Option<Tag<'a>> {
        let bytes = self.xml.as_bytes();
        loop {
            let start = self.pos + self.xml[self.pos..].find('<')?;
            let rest = &self.xml[start..];
            let skip_to = |pat: &str| rest.find(pat).map(|i| start + i + pat.len());
            if rest.starts_with("<?") {
                self.pos = skip_to("?>")?;
                continue;
            }
            if rest.starts_with("<!--") {
                self.pos = skip_to("-->")?;
                continue;
            }
            if rest.starts_with("<![CDATA[") {
                self.pos = skip_to("]]>")?;
                continue;
            }
            if rest.starts_with("<!") {
                self.pos = skip_to(">")?;
                continue;
            }
            let mut i = start + 1;
            let mut quote: Option<u8> = None;
            while i < bytes.len() {
                let b = bytes[i];
                match quote {
                    Some(q) if b == q => quote = None,
                    Some(_) => {}
                    None if b == b'"' || b == b'\'' => quote = Some(b),
                    None if b == b'>' => break,
                    None => {}
                }
                i += 1;
            }
            if i >= bytes.len() {
                return None;
            }
            let end = i + 1;
            self.pos = end;
            let inner = &self.xml[start + 1..i];
            let (kind, inner) = if let Some(s) = inner.strip_prefix('/') {
                (Kind::Close, s)
            } else if let Some(s) = inner.strip_suffix('/') {
                (Kind::Empty, s)
            } else {
                (Kind::Open, inner)
            };
            let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
            return Some(Tag {
                kind,
                name: &inner[..name_end],
                attrs: &inner[name_end..],
                start,
                end,
            });
        }
    }
}

pub fn attr(attrs: &str, key: &str) -> Option<String> {
    let mut rest = attrs;
    while let Some(eq) = rest.find('=') {
        let k = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let q = after.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let close = after[1..].find(q)? + 1;
        if k == key {
            return Some(unescape(&after[1..close]));
        }
        rest = &after[close + 1..];
    }
    None
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let Some(semi) = after.find(';') else {
            out.push_str(after);
            return out;
        };
        let entity = &after[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&after[..=semi]),
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_tags_with_offsets() {
        let xml = r#"<?xml version="1.0"?><A x="1>2"><!-- c --><B/><C k='v'>t &amp; u</C></A>"#;
        let t: Vec<_> = tags(xml).map(|t| (t.kind, t.name)).collect();
        assert_eq!(
            t,
            vec![(Kind::Open, "A"), (Kind::Empty, "B"), (Kind::Open, "C"), (Kind::Close, "C"), (Kind::Close, "A")]
        );
        let a = tags(xml).next().unwrap();
        assert_eq!(attr(a.attrs, "x").as_deref(), Some("1>2"));
        assert_eq!(&xml[a.start..a.end], r#"<A x="1>2">"#);
        assert_eq!(unescape("t &amp; u &#65;"), "t & u A");
        assert_eq!(escape("a&b<\""), "a&amp;b&lt;&quot;");
    }
}
