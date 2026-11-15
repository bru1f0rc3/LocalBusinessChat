//! Emoticon packs in Miranda SmileyAdd ".asl"/".msl" format (as used by the
//! original program's Emoticons folder). Animated GIFs are supported.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use image::AnimationDecoder;
use std::path::{Path, PathBuf};

pub struct Emo {
    pub codes: Vec<String>,
    pub tip: String,
    pub frames: Vec<(TextureHandle, u32)>,
    pub total_ms: u32,
    pub size: egui::Vec2,
}

#[derive(Default)]
pub struct Pack {
    pub name: String,
    pub author: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    pub items: Vec<Emo>,
    /// (code, item index), longest code first
    index: Vec<(String, usize)>,
}

pub enum Seg<'a> {
    Text(&'a str),
    Emo(usize),
    Url(&'a str),
}

fn decode_entities(s: &str) -> String {
    s.replace("&gt;", ">").replace("&lt;", "<").replace("&quot;", "\"").replace("&amp;", "&")
}

/// Split `Smiley = "file", 0, "codes", "tip"` arguments.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut had_q = false;
    for c in s.chars() {
        match c {
            '"' => {
                in_q = !in_q;
                had_q = true;
            }
            ',' if !in_q => {
                out.push(if had_q { cur.clone() } else { cur.trim().to_string() });
                cur.clear();
                had_q = false;
            }
            _ => {
                if in_q || !c.is_whitespace() {
                    cur.push(c)
                }
            }
        }
    }
    if !cur.is_empty() || had_q {
        out.push(if had_q { cur } else { cur.trim().to_string() });
    }
    out
}

fn load_frames(path: &Path) -> Option<Vec<(ColorImage, u32)>> {
    let bytes = std::fs::read(path).ok()?;
    let is_gif = bytes.starts_with(b"GIF8");
    if is_gif {
        if let Ok(dec) = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&bytes)) {
            if let Ok(frames) = dec.into_frames().collect_frames() {
                let v: Vec<(ColorImage, u32)> = frames
                    .into_iter()
                    .map(|f| {
                        let (n, d) = f.delay().numer_denom_ms();
                        let ms = if d == 0 { 100 } else { (n / d).max(20) };
                        let buf = f.into_buffer();
                        let size = [buf.width() as usize, buf.height() as usize];
                        (ColorImage::from_rgba_unmultiplied(size, buf.as_raw()), ms)
                    })
                    .collect();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(vec![(ColorImage::from_rgba_unmultiplied(size, img.as_raw()), 0)])
}

impl Pack {
    pub fn load(ctx: &egui::Context, asl: &Path) -> Result<Pack, String> {
        let raw = std::fs::read(asl).map_err(|e| e.to_string())?;
        let text = match String::from_utf8(raw.clone()) {
            Ok(s) => s,
            Err(_) => raw.iter().map(|&b| cp1251(b)).collect(),
        };
        let dir = asl.parent().unwrap_or(Path::new("."));
        let mut pack = Pack { path: asl.to_path_buf(), ..Default::default() };
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with(';') || line.is_empty() {
                continue;
            }
            let Some((key, val)) = line.split_once('=') else { continue };
            let key = key.trim().to_lowercase();
            let val = val.trim();
            match key.as_str() {
                "name" => pack.name = val.trim_matches('"').to_string(),
                "author" => pack.author = val.trim_matches('"').to_string(),
                "smiley" => {
                    let a = split_args(val);
                    if a.len() < 3 {
                        continue;
                    }
                    let file = a[0].trim().trim_start_matches(".\\").trim_start_matches("./").replace('\\', "/");
                    let codes: Vec<String> = a[2].split_whitespace().map(decode_entities).collect();
                    if codes.is_empty() {
                        continue;
                    }
                    let tip = a.get(3).cloned().unwrap_or_else(|| codes[0].clone());
                    let Some(frames) = load_frames(&dir.join(&file)) else { continue };
                    let n = pack.items.len();
                    let size = egui::vec2(frames[0].0.size[0] as f32, frames[0].0.size[1] as f32);
                    let total_ms = frames.iter().map(|f| f.1).sum();
                    let frames = frames
                        .into_iter()
                        .enumerate()
                        .map(|(i, (img, ms))| {
                            (ctx.load_texture(format!("emo:{}:{}:{}", asl.display(), n, i), img, TextureOptions::LINEAR), ms)
                        })
                        .collect();
                    pack.items.push(Emo { codes, tip, frames, total_ms, size });
                }
                _ => {}
            }
        }
        if pack.items.is_empty() {
            return Err("В пакете нет смайлов".into());
        }
        if pack.name.is_empty() {
            pack.name = asl.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        }
        let mut index = Vec::new();
        for (i, e) in pack.items.iter().enumerate() {
            for c in &e.codes {
                index.push((c.clone(), i));
            }
        }
        index.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        pack.index = index;
        Ok(pack)
    }

    /// Current animation frame texture of an emoticon.
    pub fn texture(&self, i: usize, time: f64) -> Option<(&TextureHandle, egui::Vec2)> {
        let e = self.items.get(i)?;
        if e.frames.len() <= 1 || e.total_ms == 0 {
            return e.frames.first().map(|f| (&f.0, e.size));
        }
        let mut t = ((time * 1000.0) as u64 % e.total_ms as u64) as u32;
        for f in &e.frames {
            if t < f.1 {
                return Some((&f.0, e.size));
            }
            t -= f.1;
        }
        e.frames.last().map(|f| (&f.0, e.size))
    }

    pub fn is_animated(&self) -> bool {
        self.items.iter().any(|e| e.frames.len() > 1)
    }

    fn match_at(&self, s: &str) -> Option<(usize, usize)> {
        for (code, i) in &self.index {
            if s.starts_with(code.as_str()) {
                return Some((code.len(), *i));
            }
        }
        None
    }

    /// Split text into plain text / emoticons. `max` limits emoticons count (flood protection).
    pub fn tokenize<'a>(&self, text: &'a str, max: usize, out: &mut Vec<Seg<'a>>) {
        let mut count = 0;
        let mut start = 0;
        let mut i = 0;
        let b = text.as_bytes();
        while i < text.len() {
            if !text.is_char_boundary(i) {
                i += 1;
                continue;
            }
            if count < max {
                // emoticons should not start in the middle of a word for alnum codes like "(a)"
                if let Some((len, idx)) = self.match_at(&text[i..]) {
                    if start < i {
                        out.push(Seg::Text(&text[start..i]));
                    }
                    out.push(Seg::Emo(idx));
                    count += 1;
                    i += len;
                    // repeatable emoticons: ":-)))" -> three smiles
                    if len >= 2 {
                        let last = b[i - 1];
                        if !last.is_ascii_alphanumeric() {
                            while i < b.len() && b[i] == last && count < max {
                                out.push(Seg::Emo(idx));
                                count += 1;
                                i += 1;
                            }
                        }
                    }
                    start = i;
                    continue;
                }
            }
            i += 1;
        }
        if start < text.len() {
            out.push(Seg::Text(&text[start..]));
        }
    }
}

/// Split into URL / non-URL parts, then emoticons on the non-URL parts.
pub fn segment<'a>(text: &'a str, pack: Option<&Pack>, max_emo: usize) -> Vec<Seg<'a>> {
    let mut out = Vec::new();
    let mut rest = text;
    let mut pos = 0;
    loop {
        let found = ["http://", "https://", "ftp://", "www.", "\\\\"]
            .iter()
            .filter_map(|p| rest[pos..].find(p).map(|i| pos + i))
            .filter(|&i| i == 0 || !rest.as_bytes()[i - 1].is_ascii_alphanumeric())
            .min();
        let Some(u) = found else { break };
        let end = rest[u..].find(|c: char| c.is_whitespace() || c == '"' || c == '<' || c == '>').map(|e| u + e).unwrap_or(rest.len());
        let mut end2 = end;
        while end2 > u && matches!(rest.as_bytes()[end2 - 1], b'.' | b',' | b')' | b'!' | b'?' | b';' | b':') {
            end2 -= 1;
        }
        if end2 - u < 5 {
            pos = end.max(u + 1);
            if pos >= rest.len() {
                break;
            }
            continue;
        }
        let before = &rest[..u];
        push_text(before, pack, max_emo, &mut out);
        out.push(Seg::Url(&rest[u..end2]));
        rest = &rest[end2..];
        pos = 0;
    }
    push_text(rest, pack, max_emo, &mut out);
    out
}

fn push_text<'a>(s: &'a str, pack: Option<&Pack>, max_emo: usize, out: &mut Vec<Seg<'a>>) {
    if s.is_empty() {
        return;
    }
    match pack {
        Some(p) => p.tokenize(s, max_emo, out),
        None => out.push(Seg::Text(s)),
    }
}

/// Find all *.asl / *.msl packs under the Emoticons folder.
pub fn find_packs(assets: Option<&Path>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let Some(a) = assets else { return v };
    let root = a.join("Emoticons");
    let Ok(rd) = std::fs::read_dir(&root) else { return v };
    for d in rd.flatten() {
        let p = d.path();
        let files: Vec<PathBuf> = if p.is_dir() {
            std::fs::read_dir(&p).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default()
        } else {
            vec![p]
        };
        for f in files {
            let ext = f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext == "asl" || ext == "msl" {
                v.push(f);
            }
        }
    }
    v.sort();
    v
}

fn cp1251(b: u8) -> char {
    match b {
        0..=0x7f => b as char,
        0xc0..=0xff => char::from_u32(0x410 + (b as u32 - 0xc0)).unwrap_or('?'),
        0xa8 => 'Ё',
        0xb8 => 'ё',
        _ => '?',
    }
}
