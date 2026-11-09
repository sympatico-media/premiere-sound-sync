//! Reading, editing and writing `.prproj` files (gzipped XML).
//!
//! Existing content is never re-serialized: new objects are appended and bin item lists are
//! spliced in place, so everything Premiere wrote stays byte-for-byte intact.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use regex::Regex;

use super::objects::{escape, new_uid, Writer};
use super::xml::{self, Kind};

const TEMPLATE: &str = include_str!("../../assets/template.prproj.xml");
const BIN_CLASS: &str = "dbfd6653-24da-480e-a35e-ba45e9504e4b";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinInfo {
    pub uid: String,
    pub name: String,
    /// Slash-separated path from the root, e.g. `Footage/Day 1`.
    pub path: String,
    pub depth: usize,
}

#[derive(Debug, Clone)]
struct Container {
    uid: String,
    name: String,
    span: Range<usize>,
    items: Vec<String>,
    is_root: bool,
}

#[derive(Debug, Clone)]
struct NewBin {
    uid: String,
    name: String,
    id: u64,
    items: Vec<String>,
}

pub struct Project {
    xml: String,
    root_uid: String,
    containers: Vec<Container>,
    parents: HashMap<String, String>,
    next_item_id: u64,
    writer: Writer,
    additions: BTreeMap<String, Vec<String>>,
    new_bins: Vec<NewBin>,
}

impl Project {
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let xml = if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut s = String::new();
            GzDecoder::new(&bytes[..])
                .read_to_string(&mut s)
                .context("decompressing project")?;
            s
        } else {
            String::from_utf8(bytes).context("project is not UTF-8 XML")?
        };
        Self::from_xml(xml)
    }

    /// A blank Premiere Pro project with fresh object UIDs.
    pub fn new_blank() -> Result<Self> {
        let uid_re = Regex::new(r#"ObjectUID="([0-9a-f-]{36})""#).unwrap();
        let mut xml = TEMPLATE.to_string();
        let uids: Vec<String> = uid_re.captures_iter(TEMPLATE).map(|c| c[1].to_string()).collect();
        for old in uids {
            xml = xml.replace(&old, &new_uid());
        }
        Self::from_xml(xml)
    }

    pub fn from_xml(xml: String) -> Result<Self> {
        if !xml.contains("<PremiereData") {
            bail!("not a Premiere Pro project");
        }
        let containers = index_containers(&xml)?;
        let root_uid = containers
            .iter()
            .find(|c| c.is_root)
            .map(|c| c.uid.clone())
            .ok_or_else(|| anyhow!("project has no root bin"))?;
        let mut parents = HashMap::new();
        for c in &containers {
            for item in &c.items {
                parents.insert(item.clone(), c.uid.clone());
            }
        }
        let max_object_id = Regex::new(r#"ObjectID="(\d+)""#)
            .unwrap()
            .captures_iter(&xml)
            .filter_map(|c| c[1].parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        let max_item_id = Regex::new(r"<ID>(\d{7,})</ID>")
            .unwrap()
            .captures_iter(&xml)
            .filter_map(|c| c[1].parse::<u64>().ok())
            .max()
            .unwrap_or(1_000_000);
        let next_id = Regex::new(r"<NextID>(\d+)</NextID>")
            .unwrap()
            .captures(&xml)
            .and_then(|c| c[1].parse::<u64>().ok())
            .unwrap_or(0);
        Ok(Self {
            xml,
            root_uid,
            containers,
            parents,
            next_item_id: next_id.max(max_item_id + 1),
            writer: Writer::new(max_object_id + 1),
            additions: BTreeMap::new(),
            new_bins: Vec::new(),
        })
    }

    pub fn root(&self) -> &str {
        &self.root_uid
    }

    /// All bins (excluding the root), depth-first in project order.
    pub fn bins(&self) -> Vec<BinInfo> {
        let by_uid: HashMap<&str, &Container> = self.containers.iter().map(|c| (c.uid.as_str(), c)).collect();
        let mut out = Vec::new();
        fn walk(uid: &str, prefix: &str, depth: usize, by_uid: &HashMap<&str, &Container>, out: &mut Vec<BinInfo>) {
            let Some(c) = by_uid.get(uid) else { return };
            for item in &c.items {
                if let Some(child) = by_uid.get(item.as_str()) {
                    let path = if prefix.is_empty() {
                        child.name.clone()
                    } else {
                        format!("{prefix}/{}", child.name)
                    };
                    out.push(BinInfo {
                        uid: child.uid.clone(),
                        name: child.name.clone(),
                        path: path.clone(),
                        depth,
                    });
                    walk(&child.uid, &path, depth + 1, by_uid, out);
                }
            }
        }
        walk(&self.root_uid, "", 0, &by_uid, &mut out);
        out
    }

    pub fn writer(&mut self) -> &mut Writer {
        &mut self.writer
    }

    /// Returns the child bin called `name` under `parent`, creating it if needed.
    pub fn ensure_bin(&mut self, parent: &str, name: &str) -> String {
        let existing = self
            .containers
            .iter()
            .find(|c| !c.is_root && c.name == name && self.parents.get(&c.uid).map(String::as_str) == Some(parent))
            .map(|c| c.uid.clone());
        if let Some(uid) = existing {
            return uid;
        }
        if let Some(b) = self
            .new_bins
            .iter()
            .find(|b| b.name == name && self.parents.get(&b.uid).map(String::as_str) == Some(parent))
        {
            return b.uid.clone();
        }
        let uid = new_uid();
        self.new_bins.push(NewBin {
            uid: uid.clone(),
            name: name.to_string(),
            id: self.next_item_id,
            items: Vec::new(),
        });
        self.next_item_id += 1;
        self.add_to_bin(parent, &uid);
        uid
    }

    /// Puts a project item (clip, sequence or bin) into a bin.
    pub fn add_to_bin(&mut self, bin: &str, item_uid: &str) {
        self.parents.insert(item_uid.to_string(), bin.to_string());
        if let Some(b) = self.new_bins.iter_mut().find(|b| b.uid == bin) {
            b.items.push(item_uid.to_string());
        } else {
            self.additions.entry(bin.to_string()).or_default().push(item_uid.to_string());
        }
    }

    pub fn to_xml(&self) -> String {
        let mut xml = self.xml.clone();
        let mut splices: Vec<(Range<usize>, String)> = Vec::new();

        for c in &self.containers {
            let Some(added) = self.additions.get(&c.uid) else { continue };
            let text = &self.xml[c.span.clone()];
            let start_index = c.items.len();
            let items: String = added
                .iter()
                .enumerate()
                .map(|(i, uid)| format!("<Item Index=\"{}\" ObjectURef=\"{uid}\"/>", start_index + i))
                .collect();
            if let Some(pos) = text.find("</Items>") {
                let at = c.span.start + pos;
                splices.push((at..at, items));
            } else if let Some(m) = Regex::new(r#"<Items Version="1"\s*/>"#).unwrap().find(text) {
                let r = c.span.start + m.start()..c.span.start + m.end();
                splices.push((r, format!("<Items Version=\"1\">{items}</Items>")));
            } else {
                let close = if c.is_root { "</RootProjectItem>" } else { "</BinProjectItem>" };
                if let Some(pos) = text.rfind(close) {
                    let at = c.span.start + pos;
                    splices.push((
                        at..at,
                        format!("<ProjectItemContainer Version=\"1\"><Items Version=\"1\">{items}</Items></ProjectItemContainer>"),
                    ));
                }
            }
        }

        if let Some(m) = Regex::new(r"<NextID>\d+</NextID>").unwrap().find(&self.xml) {
            splices.push((m.range(), format!("<NextID>{}</NextID>", self.next_item_id)));
        }

        let mut appended = String::new();
        for obj in self.writer.objects() {
            appended.push('\t');
            appended.push_str(obj);
            appended.push('\n');
        }
        for b in &self.new_bins {
            let items: String = b
                .items
                .iter()
                .enumerate()
                .map(|(i, uid)| format!("<Item Index=\"{i}\" ObjectURef=\"{uid}\"/>"))
                .collect();
            appended.push_str(&format!(
                "\t<BinProjectItem ObjectUID=\"{}\" ClassID=\"{BIN_CLASS}\" Version=\"1\"><ProjectItem Version=\"1\"><Node Version=\"1\"><Properties Version=\"1\"></Properties><ID>{}</ID></Node><Name>{}</Name></ProjectItem><ProjectItemContainer Version=\"1\"><Items Version=\"1\">{items}</Items></ProjectItemContainer></BinProjectItem>\n",
                b.uid,
                b.id,
                escape(&b.name)
            ));
        }
        if let Some(pos) = self.xml.rfind("</PremiereData>") {
            splices.push((pos..pos, appended));
        }

        splices.sort_by(|a, b| b.0.start.cmp(&a.0.start));
        for (range, text) in splices {
            xml.replace_range(range, &text);
        }
        xml
    }

    /// Writes the project gzipped. If `path` exists it is first copied to `<path>.bak`.
    pub fn save(&self, path: &Path) -> Result<()> {
        if path.exists() {
            let backup = path.with_extension("prproj.bak");
            fs::copy(path, &backup).with_context(|| format!("backing up to {}", backup.display()))?;
        }
        let tmp = path.with_extension("prproj.tmp");
        {
            let file = fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
            let mut gz = GzEncoder::new(file, Compression::default());
            gz.write_all(self.to_xml().as_bytes())?;
            gz.finish()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// Finds the root bin and every bin with their spans and child item UIDs.
fn index_containers(xml: &str) -> Result<Vec<Container>> {
    let mut depth = 0usize;
    let mut out = Vec::new();
    let mut current: Option<(String, usize, bool, &str)> = None;
    let mut name: Option<String> = None;
    let mut name_start: Option<usize> = None;
    let mut items: Vec<String> = Vec::new();
    for tag in xml::tags(xml) {
        match tag.kind {
            Kind::Open => {
                depth += 1;
                if depth == 2 && (tag.name == "RootProjectItem" || tag.name == "BinProjectItem") {
                    if let Some(uid) = xml::attr(tag.attrs, "ObjectUID") {
                        current = Some((uid, tag.start, tag.name == "RootProjectItem", tag.name));
                        name = None;
                        items.clear();
                    }
                } else if current.is_some() && tag.name == "Name" && name.is_none() {
                    name_start = Some(tag.end);
                }
            }
            Kind::Empty => {
                if current.is_some() && tag.name == "Item" {
                    if let Some(u) = xml::attr(tag.attrs, "ObjectURef") {
                        items.push(u);
                    }
                }
            }
            Kind::Close => {
                if tag.name == "Name" {
                    if let Some(s) = name_start.take() {
                        name = Some(xml::unescape(&xml[s..tag.start]));
                    }
                }
                if depth == 2 {
                    if let Some((uid, start, is_root, open_name)) = current.take() {
                        if tag.name == open_name {
                            out.push(Container {
                                uid,
                                name: name.take().unwrap_or_default(),
                                span: start..tag.end,
                                items: std::mem::take(&mut items),
                                is_root,
                            });
                        }
                    }
                }
                depth = depth.saturating_sub(1);
            }
        }
    }
    if depth != 0 {
        bail!("project XML is truncated or malformed");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_project_has_root_and_no_bins() {
        let p = Project::new_blank().unwrap();
        assert!(!p.root().is_empty());
        assert!(p.bins().is_empty());
    }

    #[test]
    fn creates_nested_bins_and_round_trips() {
        let mut p = Project::new_blank().unwrap();
        let root = p.root().to_string();
        let synced = p.ensure_bin(&root, "Synced");
        let scene = p.ensure_bin(&synced, "Scene 2");
        assert_eq!(p.ensure_bin(&synced, "Scene 2"), scene);
        let xml = p.to_xml();

        let reopened = Project::from_xml(xml).unwrap();
        let bins = reopened.bins();
        assert_eq!(bins.iter().map(|b| b.path.as_str()).collect::<Vec<_>>(), vec!["Synced", "Synced/Scene 2"]);

        let mut again = reopened;
        let synced_uid = again.bins()[0].uid.clone();
        assert_eq!(again.ensure_bin(&synced_uid, "Scene 2"), again.bins()[1].uid);
        let extra = again.ensure_bin(&synced_uid, "Scene 3");
        let xml = again.to_xml();
        let third = Project::from_xml(xml).unwrap();
        assert_eq!(third.bins().len(), 3);
        assert!(third.bins().iter().any(|b| b.uid == extra && b.path == "Synced/Scene 3"));
    }
}
