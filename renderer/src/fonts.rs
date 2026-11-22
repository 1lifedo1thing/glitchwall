//! The bundled fonts (OFL, in the wallpaper folder) as a private DirectWrite collection: nothing is installed and no
//! system font stands in for them. Families are found by the names the page's CSS uses.
use std::path::Path;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::*;

pub struct Fonts { pub dw: IDWriteFactory5, pub set: IDWriteFontCollection1 }

impl Fonts {
    pub fn load(wallpaper: &Path) -> Result<Fonts> {
        unsafe {
            let dw: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let builder: IDWriteFontSetBuilder1 = dw.CreateFontSetBuilder()?;
            let mut files: Vec<_> = std::fs::read_dir(wallpaper).into_iter().flatten()
                .chain(std::fs::read_dir(wallpaper.join("fonts")).into_iter().flatten())
                .flatten().map(|e| e.path())
                .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("ttf" | "otf"))).collect();
            files.sort();
            for f in files {
                let wide: Vec<u16> = f.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
                let file = dw.CreateFontFileReference(PCWSTR(wide.as_ptr()), None)?;
                builder.AddFontFile(&file)?;
            }
            let set = builder.CreateFontSet()?;
            let set = dw.CreateFontCollectionFromFontSet(&set)?;
            Ok(Fonts { dw, set })
        }
    }

    pub fn has(&self, family: &str) -> bool {
        let w: Vec<u16> = family.encode_utf16().chain([0]).collect();
        let (mut i, mut found) = (0u32, BOOL(0));
        unsafe { self.set.FindFamilyName(PCWSTR(w.as_ptr()), &mut i, &mut found).is_ok() && found.as_bool() }
    }

    /// a text format in a bundled family; a family that isn't bundled (a system-font menu choice) comes from Windows
    pub fn format(&self, family: &str, size: f32, weight: u32) -> Result<IDWriteTextFormat> {
        let w: Vec<u16> = family.encode_utf16().chain([0]).collect();
        let coll: Option<IDWriteFontCollection> = if self.has(family) { Some(self.set.cast()?) } else { None };
        unsafe {
            self.dw.CreateTextFormat(PCWSTR(w.as_ptr()), coll.as_ref(), DWRITE_FONT_WEIGHT(weight as i32), DWRITE_FONT_STYLE_NORMAL,
                                     DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_theme_font_is_bundled() {
        let wp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/glitchwall/wallpaper");
        let f = Fonts::load(&wp).unwrap();
        for fam in ["IBM Plex Mono", "VT323", "Departure Mono", "Chakra Petch", "Doto", "Saira Stencil One", "Instrument Serif",
                    "Gelasio", "Courier Prime", "DM Mono", "Fira Code", "Overpass Mono", "Cascadia Mono", "Inconsolata", "Caveat"] {
            assert!(f.has(fam), "{fam} not found in the bundled fonts");
        }
        assert!(!f.has("Segoe UI"));
    }
}
