//! A small PDF writer for the family letter and the recovery packet. It needs no new crate and
//! works the same on every platform, Android included.
//!
//! Input is the Markdown Nakos already produces: `#`/`##`/`###` headings, `-` and `1.` lists
//! (indented for a sub-list), `>` quotes, fenced code blocks, and paragraphs. `**bold**`, `*italic*`
//! and `` `code` `` markers are dropped and the text kept. The output is A4 in the standard PDF
//! fonts (Helvetica, Helvetica-Bold, Courier), so no font file is embedded, and every page carries
//! a footer: the title, "contains no seed or private key", and "page n of N". Long tokens (keys,
//! addresses) are broken across lines rather than running off the page.

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 56.0;
const BOTTOM: f32 = 72.0;
const TEXT_W: f32 = PAGE_W - 2.0 * MARGIN;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Font {
    Regular,
    Bold,
    Mono,
}

impl Font {
    fn name(self) -> &'static str {
        match self {
            Font::Regular => "F1",
            Font::Bold => "F2",
            Font::Mono => "F3",
        }
    }
}

/// Helvetica and Helvetica-Bold advance widths (1/1000 em) for ASCII 32..=126, from the standard
/// Adobe font metrics. Courier is 600 for every character.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667,
    667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500,
    556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722,
    722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556,
    611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// The WinAnsi byte for a character, or `?` when the standard fonts can't draw it.
fn win_ansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '€' => 0x80,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '→' => b'>',
        _ => b'?',
    }
}

fn char_width(b: u8, font: Font) -> f32 {
    let w = match font {
        Font::Mono => 600,
        Font::Regular | Font::Bold => {
            let table = if font == Font::Bold { &HELV_BOLD } else { &HELV };
            match b {
                32..=126 => table[(b - 32) as usize],
                0x85 | 0x97 => 1000,
                0x95 => 350,
                0x91 | 0x92 => 222,
                0x93 | 0x94 => 333,
                _ => 556,
            }
        }
    };
    f32::from(w) / 1000.0
}

fn text_width(bytes: &[u8], font: Font, size: f32) -> f32 {
    bytes.iter().map(|&b| char_width(b, font)).sum::<f32>() * size
}

/// Drops the inline Markdown markers Nakos uses, keeping the text.
fn plain(s: &str) -> String {
    s.replace("**", "").replace('`', "").replace("\\*", "*").trim_matches('*').to_string()
}

/// Breaks text into lines that fit `width`. Words longer than a line are cut.
fn wrap(text: &str, font: Font, size: f32, width: f32) -> Vec<Vec<u8>> {
    let bytes: Vec<u8> = text.chars().map(win_ansi).collect();
    let mut lines = Vec::new();
    let mut line: Vec<u8> = Vec::new();
    for word in bytes.split(|&b| b == b' ').filter(|w| !w.is_empty()) {
        let mut word = word.to_vec();
        loop {
            let candidate_w = if line.is_empty() { text_width(&word, font, size) } else { text_width(&line, font, size) + text_width(b" ", font, size) + text_width(&word, font, size) };
            if candidate_w <= width {
                if !line.is_empty() {
                    line.push(b' ');
                }
                line.extend_from_slice(&word);
                break;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                continue;
            }
            // A single word wider than the line: cut it where it stops fitting.
            let mut cut = 1;
            while cut < word.len() && text_width(&word[..cut + 1], font, size) <= width {
                cut += 1;
            }
            lines.push(word[..cut].to_vec());
            word = word[cut..].to_vec();
            if word.is_empty() {
                break;
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

struct Layout {
    pages: Vec<String>,
    current: String,
    y: f32,
}

impl Layout {
    fn new() -> Self {
        Layout { pages: Vec::new(), current: String::new(), y: PAGE_H - MARGIN }
    }
    fn need(&mut self, h: f32) {
        if self.y - h < BOTTOM {
            self.pages.push(std::mem::take(&mut self.current));
            self.y = PAGE_H - MARGIN;
        }
    }
    fn line(&mut self, x: f32, bytes: &[u8], font: Font, size: f32, leading: f32) {
        self.need(leading);
        self.y -= leading;
        self.current.push_str(&format!("BT /{} {size} Tf {x:.1} {:.1} Td <{}> Tj ET\n", font.name(), self.y, hex(bytes)));
    }
    fn para(&mut self, text: &str, font: Font, size: f32, indent: f32, bullet: Option<&str>) {
        let leading = size * 1.38;
        let x = MARGIN + indent;
        let lines = wrap(text, font, size, TEXT_W - indent - if bullet.is_some() { 14.0 } else { 0.0 });
        for (i, l) in lines.iter().enumerate() {
            if i == 0 {
                if let Some(b) = bullet {
                    self.need(leading);
                    let bytes: Vec<u8> = b.chars().map(win_ansi).collect();
                    self.current.push_str(&format!("BT /F1 {size} Tf {x:.1} {:.1} Td <{}> Tj ET\n", self.y - leading, hex(&bytes)));
                }
            }
            self.line(x + if bullet.is_some() { 14.0 } else { 0.0 }, l, font, size, leading);
        }
    }
    fn gap(&mut self, h: f32) {
        self.y -= h;
    }
    fn rule(&mut self) {
        self.need(8.0);
        self.y -= 4.0;
        self.current.push_str(&format!("0.75 G 0.5 w {MARGIN:.1} {y:.1} m {:.1} {y:.1} l S 0 G\n", PAGE_W - MARGIN, y = self.y));
        self.y -= 4.0;
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Lays the Markdown out on pages. Returns one content stream per page (footer not yet added).
fn layout(markdown: &str) -> Vec<String> {
    let mut l = Layout::new();
    let mut in_code = false;
    for raw in markdown.lines() {
        let line = raw.trim_end();
        if line.starts_with("```") {
            in_code = !in_code;
            l.gap(4.0);
            continue;
        }
        if in_code {
            for w in wrap(line, Font::Mono, 7.5, TEXT_W - 8.0) {
                l.line(MARGIN + 8.0, &w, Font::Mono, 7.5, 9.5);
            }
            continue;
        }
        let trimmed = line.trim_start();
        let indent = ((line.len() - trimmed.len()) / 2) as f32 * 14.0;
        if trimmed.is_empty() {
            l.gap(5.0);
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            l.need(40.0);
            l.gap(6.0);
            l.para(&plain(h), Font::Bold, 18.0, 0.0, None);
            l.rule();
        } else if let Some(h) = trimmed.strip_prefix("## ") {
            l.need(36.0);
            l.gap(8.0);
            l.para(&plain(h), Font::Bold, 13.0, 0.0, None);
        } else if let Some(h) = trimmed.strip_prefix("### ") {
            l.need(30.0);
            l.gap(4.0);
            l.para(&plain(h), Font::Bold, 11.0, 0.0, None);
        } else if let Some(item) = trimmed.strip_prefix("- [ ] ") {
            l.para(&plain(item), Font::Regular, 10.0, indent, Some("[  ]"));
        } else if let Some(item) = trimmed.strip_prefix("- ") {
            let bold = item.starts_with("**") && item.contains("**:");
            l.para(&plain(item), if bold { Font::Bold } else { Font::Regular }, 10.0, indent, Some("\u{2022}"));
        } else if let Some((n, rest)) = trimmed.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
            l.para(&plain(rest), Font::Regular, 10.0, indent, Some(&format!("{n}.")));
        } else if let Some(q) = trimmed.strip_prefix("> ") {
            l.para(&plain(q), Font::Bold, 10.0, 10.0, None);
        } else if trimmed.starts_with("**") && trimmed.ends_with("**") {
            l.gap(3.0);
            l.para(&plain(trimmed), Font::Bold, 10.0, indent, None);
        } else {
            l.para(&plain(trimmed), Font::Regular, 10.0, indent, None);
        }
    }
    l.pages.push(l.current);
    l.pages.retain(|p| !p.is_empty());
    if l.pages.is_empty() {
        l.pages.push(String::new());
    }
    l.pages
}

/// Renders `markdown` to a complete PDF file. `title` goes in the footer and the document info.
pub fn markdown_to_pdf(title: &str, markdown: &str) -> Vec<u8> {
    let pages = layout(markdown);
    let n = pages.len();
    // Objects: 1 catalog, 2 pages, 3-5 fonts, 6 info, then (page, content) pairs.
    let mut objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Count {n} /Kids [{}] >>", (0..n).map(|i| format!("{} 0 R", 7 + 2 * i)).collect::<Vec<_>>().join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".into(),
        format!("<< /Title <FEFF{}> /Producer (Nakos) >>", title.encode_utf16().map(|u| format!("{u:04X}")).collect::<String>()),
    ];
    let title_bytes: Vec<u8> = title.chars().map(win_ansi).collect();
    for (i, body) in pages.iter().enumerate() {
        let footer_left = hex(&title_bytes);
        let note: Vec<u8> = "Watch-only. Contains no seed words or private keys.".chars().map(win_ansi).collect();
        let page_no = format!("Page {} of {n}", i + 1);
        let pw = text_width(page_no.as_bytes(), Font::Regular, 8.0);
        let stream = format!(
            "{body}0.4 g\nBT /F1 8 Tf {MARGIN:.1} 40 Td <{footer_left}> Tj ET\nBT /F1 8 Tf {MARGIN:.1} 30 Td <{}> Tj ET\nBT /F1 8 Tf {:.1} 40 Td ({page_no}) Tj ET\n0 g\n",
            hex(&note),
            PAGE_W - MARGIN - pw
        );
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> /Contents {} 0 R >>",
            8 + 2 * i
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()));
    }
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{obj}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(pdf: &[u8]) -> String {
        // Every drawn string is hex; decode them all back to text to check what's on the page.
        let s = String::from_utf8_lossy(pdf);
        let mut out = String::new();
        for part in s.split('<').skip(1) {
            let Some(end) = part.find("> Tj") else { continue };
            let h = &part[..end];
            let bytes: Vec<u8> = (0..h.len()).step_by(2).filter_map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok()).collect();
            out.push_str(&String::from_utf8_lossy(&bytes));
            out.push('\n');
        }
        out
    }

    #[test]
    fn a_pdf_is_well_formed_with_a_correct_xref() {
        let pdf = markdown_to_pdf("Letter", "# Title\n\nHello.\n");
        assert!(pdf.starts_with(b"%PDF-1.4\n") && pdf.ends_with(b"%%EOF\n"));
        let tail = String::from_utf8(pdf[pdf.len() - 40..].to_vec()).unwrap();
        let startxref: usize = tail.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(pdf[startxref..].starts_with(b"xref\n"), "startxref points at the table");
        let table = String::from_utf8(pdf[startxref..].to_vec()).unwrap();
        let mut checked = 0;
        for (i, line) in table.lines().skip(3).take_while(|l| l.ends_with(" n ")).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(pdf[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
            checked += 1;
        }
        assert_eq!(checked, 8, "catalog, pages, 3 fonts, info, one page and its content");
    }

    #[test]
    fn long_text_breaks_into_lines_and_pages_and_keeps_every_word() {
        let para = "word ".repeat(2000);
        let pdf = markdown_to_pdf("Packet", &format!("# Big\n\n{para}\n"));
        let s = String::from_utf8_lossy(&pdf);
        let pages: usize = s.split("/Count ").nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap();
        assert!(pages >= 3, "{pages} pages");
        assert!(s.contains(&format!("Page {pages} of {pages}")));
        let t = text_of(&pdf);
        assert_eq!(t.matches("word").count() - t.matches("seed words").count(), 2000);
    }

    #[test]
    fn a_long_key_is_cut_to_fit_and_nothing_is_lost() {
        let key = "xpub6CUGRUonZSQ4TWtTMmzXdrXDtypWKiKrhko4egpiMZbpiaQL2jkwSB1icqYh2cfDfVxdx4df189oLKnC5fSwqPfgyP3hooxujYzAu3fDVmz";
        let lines = wrap(&key.repeat(3), Font::Mono, 7.5, 200.0);
        assert!(lines.len() > 3);
        assert!(lines.iter().all(|l| text_width(l, Font::Mono, 7.5) <= 200.0 + 0.01));
        assert_eq!(lines.concat(), key.repeat(3).into_bytes());
    }

    #[test]
    fn markdown_markers_are_dropped_and_typography_survives() {
        let pdf = markdown_to_pdf("T", "## What you need to do\n\n- **Savings**: last known — 0.5 BTC\n1. Keep it `safe`.\n");
        let t = text_of(&pdf);
        assert!(t.contains("What you need to do"));
        assert!(t.contains("Savings: last known \u{fffd} 0.5 BTC") || t.contains("Savings: last known"), "{t}");
        assert!(!t.contains("**") && !t.contains('`'), "{t}");
        assert!(t.contains("Keep it safe."));
        assert_eq!(win_ansi('—'), 0x97);
        assert_eq!(win_ansi('é'), 0xE9);
        assert_eq!(win_ansi('漢'), b'?');
    }

    #[test]
    fn every_page_carries_the_no_secrets_footer() {
        let pdf = markdown_to_pdf("Letter", &"line\n\n".repeat(400));
        let t = text_of(&pdf);
        let s = String::from_utf8_lossy(&pdf);
        let pages: usize = s.split("/Count ").nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap();
        assert_eq!(t.matches("Watch-only. Contains no seed words or private keys.").count(), pages);
    }
}
