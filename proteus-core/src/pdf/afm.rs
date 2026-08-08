//! Helvetica family metrics from the AFM files (assets/afm/) for text layout:
//! watermark/page-number centering and HTML→PDF. The 14 standard fonts are not
//! embedded; viewers substitute their built-in face, so layout must use the
//! *real* Helvetica widths (the AFM), not guesses.


/// Font variants supported for layout math.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelveticaFace {
    Regular,
    Bold,
    Oblique,
    BoldOblique,
}

impl HelveticaFace {
    fn afm_resource(self) -> &'static str {
        match self {
            HelveticaFace::Regular => include_str!("../assets/afm/Helvetica.afm"),
            HelveticaFace::Bold => include_str!("../assets/afm/Helvetica-Bold.afm"),
            HelveticaFace::Oblique => include_str!("../assets/afm/Helvetica-Oblique.afm"),
            HelveticaFace::BoldOblique => include_str!("../assets/afm/Helvetica-BoldOblique.afm"),
        }
    }
}

/// Width table in 1/1000 em indexed by byte (WinAnsi byte value).
pub struct FaceMetrics {
    widths: [u16; 256],
}

fn parse_afm(afm: &str) -> [u16; 256] {
    let mut widths = [500u16; 256];
    for line in afm.lines() {
        // "C 32 ; WX 278 ; N space ;"
        let mut it = line.split_whitespace();
        if it.next() != Some("C") {
            continue;
        }
        let code: u32 = match it.next().and_then(|t| t.parse().ok()) {
            Some(c) => c,
            None => continue,
        };
        if code > 255 {
            continue;
        }
        let mut wx = None;
        while let Some(tok) = it.next() {
            if tok == "WX" {
                wx = it.next().and_then(|t| t.parse::<f32>().ok().map(|w| w.round() as u16));
                break;
            }
        }
        if let Some(w) = wx {
            widths[code as usize] = w.max(1);
        }
    }
    widths
}

static METRICS: std::sync::OnceLock<[FaceMetrics; 4]> = std::sync::OnceLock::new();

/// Width of a single char in 1/1000 em.
pub fn char_width_em(face: HelveticaFace, b: u8) -> u16 {
    let all = METRICS.get_or_init(|| {
        let mut faces = [
            FaceMetrics { widths: parse_afm(HelveticaFace::Regular.afm_resource()) },
            FaceMetrics { widths: parse_afm(HelveticaFace::Bold.afm_resource()) },
            FaceMetrics { widths: parse_afm(HelveticaFace::Oblique.afm_resource()) },
            FaceMetrics { widths: parse_afm(HelveticaFace::BoldOblique.afm_resource()) },
        ];
        // parse_afm indexes by WinAnsi byte; faces is array of 4 — build
        // by index so HelveticaFace::* as usize maps 1:1.
        let faces_list = [HelveticaFace::Regular, HelveticaFace::Bold, HelveticaFace::Oblique, HelveticaFace::BoldOblique];
        for (i, f) in faces_list.iter().enumerate() {
            faces[i] = FaceMetrics { widths: parse_afm(f.afm_resource()) };
        }
        faces
    });
    all[face as usize].widths[b as usize]
}

/// Width of `text` (already WinAnsi bytes) in points at `size` pt.
pub fn text_width_pt(text: &[u8], size: f32) -> f32 {
    text.iter().map(|&b| f32::from(char_width_em(HelveticaFace::Regular, b))).sum::<f32>() * size / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AFM must parse to the well-known Helvetica advance widths.
    #[test]
    fn afm_parses_known_advances() {
        // Helvetica AFM: space=278, A=667, m=833, W=944 (per the Standard 14 spec).
        assert_eq!(char_width_em(HelveticaFace::Regular, b' '), 278);
        assert_eq!(char_width_em(HelveticaFace::Regular, b'A'), 667);
        assert_eq!(char_width_em(HelveticaFace::Regular, b'm'), 833);
        assert_eq!(char_width_em(HelveticaFace::Regular, b'W'), 944);
    }

    /// Widths are proportional to font size.
    #[test]
    fn width_scales_with_size() {
        let small = text_width_pt(b"M", 10.0);
        let large = text_width_pt(b"M", 20.0);
        assert!((large - small * 2.0).abs() < 0.01);
    }

    /// Bold must be wider than regular on digits (a layout-sensitive pair).
    #[test]
    fn bold_is_wider_on_upright() {
        assert!(
            char_width_em(HelveticaFace::Bold, b'A') > char_width_em(HelveticaFace::Regular, b'A')
        );
        assert_eq!(char_width_em(HelveticaFace::Bold, b'1'), char_width_em(HelveticaFace::Regular, b'1'));
    }
}

/// Centering offset helper (test oracle support).
#[cfg(test)]
pub(crate) fn center_x_offset(text: &[u8], size: f32, box_width: f32) -> f32 {
    let w = text_width_pt(text, size);
    (box_width - w) / 2.0
}

#[cfg(test)]
mod center_tests {
    use super::*;

    #[test]
    fn centering_puts_middle_of_text_at_box_middle() {
        let w = text_width_pt(b"AB", 10.0);
        let left = center_x_offset(b"AB", 10.0, 600.0);
        assert!((left + w / 2.0 - 300.0).abs() < 1e-3);
    }
}