//! Reads Broadcast WAV metadata (`fmt `, `bext`, `iXML`) straight from the RIFF chunks.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::media::Slate;

#[derive(Debug, Clone, Default)]
pub struct WavInfo {
    pub channels: u32,
    pub sample_rate: u32,
    pub block_align: u32,
    pub data_bytes: u64,
    /// Samples since midnight (BWF `TimeReference`).
    pub time_reference: Option<u64>,
    pub description: String,
    pub ixml: Option<String>,
}

impl WavInfo {
    pub fn total_samples(&self) -> u64 {
        if self.block_align == 0 {
            0
        } else {
            self.data_bytes / self.block_align as u64
        }
    }

    pub fn slate(&self) -> Slate {
        let ixml = self.ixml.as_deref().unwrap_or("");
        let scene_raw = xml_tag(ixml, "SCENE").or_else(|| desc_value(&self.description, "sSCENE"));
        let take = xml_tag(ixml, "TAKE").or_else(|| desc_value(&self.description, "sTAKE"));
        let circled = xml_tag(ixml, "CIRCLED")
            .or_else(|| desc_value(&self.description, "sCIRCLED"))
            .is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let (scene, shot) = match scene_raw {
            Some(raw) => crate::naming::split_scene(&raw),
            None => (None, None),
        };
        Slate {
            scene,
            shot,
            take,
            circled,
        }
    }

    /// Track names in channel order. iXML `TRACK_LIST` wins; bext `sTRKn` is the fallback.
    pub fn channel_names(&self) -> Vec<String> {
        let mut names = vec![String::new(); self.channels as usize];
        if let Some(ixml) = &self.ixml {
            let track_re = Regex::new(r"(?s)<TRACK>(.*?)</TRACK>").unwrap();
            for cap in track_re.captures_iter(ixml) {
                let body = &cap[1];
                let index = xml_tag(body, "CHANNEL_INDEX").and_then(|v| v.parse::<usize>().ok());
                let name = xml_tag(body, "NAME").unwrap_or_default();
                if let Some(i) = index.filter(|i| (1..=names.len()).contains(i)) {
                    names[i - 1] = name;
                }
            }
            if names.iter().any(|n| !n.is_empty()) {
                return names;
            }
        }
        let trk_re = Regex::new(r"sTRK(\d+)=([^\r\n]*)").unwrap();
        let mut tracks: Vec<(u32, String)> = trk_re
            .captures_iter(&self.description)
            .filter_map(|c| Some((c[1].parse().ok()?, c[2].trim().to_string())))
            .collect();
        tracks.sort_by_key(|t| t.0);
        for (slot, (_, name)) in names.iter_mut().zip(tracks) {
            *slot = name;
        }
        names
    }
}

fn desc_value(description: &str, key: &str) -> Option<String> {
    description.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k == key && !v.trim().is_empty()).then(|| v.trim().to_string())
    })
}

fn xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = start + xml[start..].find(&close)?;
    let value = xml[start..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

pub fn read_wav(path: &Path) -> Result<WavInfo> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut r = BufReader::new(file);
    let mut header = [0u8; 12];
    r.read_exact(&mut header)?;
    let riff = &header[0..4];
    if (riff != b"RIFF" && riff != b"RF64" && riff != b"BW64") || &header[8..12] != b"WAVE" {
        bail!("not a WAV file");
    }
    let mut info = WavInfo::default();
    let mut ds64_data_size: Option<u64> = None;
    loop {
        let mut chunk = [0u8; 8];
        if r.read_exact(&mut chunk).is_err() {
            break;
        }
        let id = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as u64;
        let padded = size + (size & 1);
        match &id {
            b"ds64" => {
                let buf = read_n(&mut r, size)?;
                if buf.len() >= 16 {
                    ds64_data_size = Some(u64::from_le_bytes(buf[8..16].try_into().unwrap()));
                }
                skip_pad(&mut r, size)?;
            }
            b"fmt " => {
                let buf = read_n(&mut r, size)?;
                if buf.len() >= 14 {
                    info.channels = u16::from_le_bytes([buf[2], buf[3]]) as u32;
                    info.sample_rate = u32::from_le_bytes(buf[4..8].try_into().unwrap());
                    info.block_align = u16::from_le_bytes([buf[12], buf[13]]) as u32;
                }
                skip_pad(&mut r, size)?;
            }
            b"bext" => {
                let buf = read_n(&mut r, size)?;
                if buf.len() >= 346 {
                    info.description = latin1(&buf[0..256]);
                    let low = u32::from_le_bytes(buf[338..342].try_into().unwrap()) as u64;
                    let high = u32::from_le_bytes(buf[342..346].try_into().unwrap()) as u64;
                    info.time_reference = Some((high << 32) | low);
                }
                skip_pad(&mut r, size)?;
            }
            b"iXML" => {
                let buf = read_n(&mut r, size)?;
                info.ixml = Some(String::from_utf8_lossy(&buf).trim_end_matches('\0').to_string());
                skip_pad(&mut r, size)?;
            }
            b"data" => {
                info.data_bytes = match ds64_data_size {
                    Some(s) if size == 0xFFFF_FFFF => s,
                    _ => size,
                };
                let skip = info.data_bytes + (info.data_bytes & 1);
                if r.seek(SeekFrom::Current(skip as i64)).is_err() {
                    break;
                }
            }
            _ => {
                if r.seek(SeekFrom::Current(padded as i64)).is_err() {
                    break;
                }
            }
        }
    }
    if info.channels == 0 || info.sample_rate == 0 {
        bail!("WAV has no fmt chunk");
    }
    Ok(info)
}

fn read_n(r: &mut impl Read, n: u64) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; n as usize];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn skip_pad(r: &mut (impl Read + Seek), size: u64) -> Result<()> {
    if size & 1 == 1 {
        r.seek(SeekFrom::Current(1))?;
    }
    Ok(())
}

fn latin1(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Writes a small polyphonic BWF with bext + iXML for tests.
    pub fn write_test_wav(path: &Path, channels: u16, samples: u32, time_ref: u64, desc: &str, ixml: Option<&str>) {
        let sample_rate = 48_000u32;
        let block_align = channels * 3;
        let data_len = samples * block_align as u32;
        let mut body = Vec::new();
        body.extend_from_slice(b"WAVE");
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&sample_rate.to_le_bytes());
        fmt.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
        fmt.extend_from_slice(&block_align.to_le_bytes());
        fmt.extend_from_slice(&24u16.to_le_bytes());
        push_chunk(&mut body, b"fmt ", &fmt);
        let mut bext = vec![0u8; 602];
        bext[..desc.len()].copy_from_slice(desc.as_bytes());
        bext[338..342].copy_from_slice(&(time_ref as u32).to_le_bytes());
        bext[342..346].copy_from_slice(&((time_ref >> 32) as u32).to_le_bytes());
        push_chunk(&mut body, b"bext", &bext);
        if let Some(x) = ixml {
            push_chunk(&mut body, b"iXML", x.as_bytes());
        }
        push_chunk(&mut body, b"data", &vec![0u8; data_len as usize]);
        let mut f = File::create(path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
        f.write_all(&body).unwrap();
    }

    fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(id);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
    }

    #[test]
    fn reads_bext_and_track_names() {
        let dir = std::env::temp_dir().join(format!("pss-bwf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("2A-003.WAV");
        let desc = "sSPEED=023.976-ND\r\nsTAKE=003\r\nsSCENE=2A\r\nsCIRCLED=TRUE\r\nsTRK1=MixL\r\nsTRK3=Boom\r\n";
        write_test_wav(&path, 2, 4800, 510_820_359, desc, None);
        let info = read_wav(&path).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(info.total_samples(), 4800);
        assert_eq!(info.time_reference, Some(510_820_359));
        assert_eq!(info.channel_names(), vec!["MixL", "Boom"]);
        let slate = info.slate();
        assert_eq!(slate.scene.as_deref(), Some("2"));
        assert_eq!(slate.shot.as_deref(), Some("A"));
        assert_eq!(slate.take.as_deref(), Some("003"));
        assert!(slate.circled);
    }

    #[test]
    fn ixml_wins_over_bext() {
        let dir = std::env::temp_dir().join(format!("pss-ixml-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.wav");
        let ixml = "<BWFXML><SCENE>14B</SCENE><TAKE>2</TAKE><CIRCLED>FALSE</CIRCLED><TRACK_LIST><TRACK><CHANNEL_INDEX>2</CHANNEL_INDEX><NAME>Lav</NAME></TRACK><TRACK><CHANNEL_INDEX>1</CHANNEL_INDEX><NAME>Boom</NAME></TRACK></TRACK_LIST></BWFXML>";
        write_test_wav(&path, 2, 10, 0, "sSCENE=99\r\n", Some(ixml));
        let info = read_wav(&path).unwrap();
        assert_eq!(info.channel_names(), vec!["Boom", "Lav"]);
        let slate = info.slate();
        assert_eq!(slate.scene.as_deref(), Some("14"));
        assert_eq!(slate.shot.as_deref(), Some("B"));
        assert!(!slate.circled);
    }
}
