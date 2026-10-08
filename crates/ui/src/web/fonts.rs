//! The web app's fonts, embedded. JetBrains Mono (the body font, three weights) and Archivo Black (headings).

use egui::{FontData, FontDefinitions, FontFamily};

pub(super) const JB_400_BYTES: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
pub(super) const JB_700_BYTES: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf");
pub(super) const JB_800_BYTES: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-ExtraBold.ttf");
pub(super) const ARCHIVO_BYTES: &[u8] = include_bytes!("../../assets/fonts/ArchivoBlack-Regular.ttf");

/// Family names used by [`super::text::Style`].
pub const MONO_400: &str = "jb400";
pub const MONO_700: &str = "jb700";
pub const MONO_800: &str = "jb800";
pub const DISPLAY: &str = "archivo";

/// The definitions: each named family lists its own face first and egui's built-in fonts after it, so a character the
/// web font lacks (an arrow, say) still renders instead of showing a box.
pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallback: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (key, bytes, family) in [("jb400", JB_400_BYTES, MONO_400), ("jb700", JB_700_BYTES, MONO_700), ("jb800", JB_800_BYTES, MONO_800), ("archivo", ARCHIVO_BYTES, DISPLAY)] {
        defs.font_data.insert(key.to_owned(), FontData::from_static(bytes));
        let mut list = vec![key.to_owned()];
        list.extend(fallback.iter().cloned());
        defs.families.insert(FontFamily::Name(family.into()), list);
    }
    // egui's own widgets (anything not drawn by this kit) should also use the web body font.
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        defs.families.entry(fam).or_default().insert(0, "jb400".to_owned());
    }
    defs
}

/// Installs the fonts. Call once, when the egui context is created.
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_four_faces_are_embedded_and_registered() {
        let d = definitions();
        for name in [MONO_400, MONO_700, MONO_800, DISPLAY] {
            let list = d.families.get(&FontFamily::Name(name.into())).unwrap_or_else(|| panic!("family {name} missing"));
            assert!(!list.is_empty());
            assert!(d.font_data.contains_key(list[0].as_str()), "{name}'s own face is not registered");
        }
    }

    #[test]
    fn the_embedded_files_are_real_truetype_fonts() {
        for (name, bytes) in [("400", JB_400_BYTES), ("700", JB_700_BYTES), ("800", JB_800_BYTES), ("archivo", ARCHIVO_BYTES)] {
            assert!(bytes.len() > 10_000, "{name} is suspiciously small");
            assert_eq!(&bytes[0..4], &[0x00, 0x01, 0x00, 0x00], "{name} is not a TrueType file");
        }
    }

    #[test]
    fn the_body_face_is_the_default_for_egui_widgets() {
        let d = definitions();
        assert_eq!(d.families[&FontFamily::Proportional][0], "jb400");
        assert_eq!(d.families[&FontFamily::Monospace][0], "jb400");
    }
}
