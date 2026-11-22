//! Themes from wallpaper/looks.js, resolved the way the page's applyLook() does: theme, borrowed palette,
//! background, font and transition, with the same defaults for what a theme leaves out.
use crate::json::Json;
use std::path::Path;

pub type Color = [f32; 4];

pub fn color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        if h.len() != 6 { return None; }
        let v = u32::from_str_radix(h, 16).ok()?;
        return Some([((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, 1.0]);
    }
    let inner = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb("))?.strip_suffix(')')?;
    let n: Vec<f32> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    match n.len() { 3 => Some([n[0] / 255.0, n[1] / 255.0, n[2] / 255.0, 1.0]), 4 => Some([n[0] / 255.0, n[1] / 255.0, n[2] / 255.0, n[3]]), _ => None }
}

pub fn with_alpha(c: Color, a: f32) -> Color { [c[0], c[1], c[2], a] }

#[derive(Clone, Debug)]
pub struct Font { pub family: String, pub scale: f32, pub weight: u32 }

/// the card glass: CSS backdrop-filter blur(), saturate(), brightness()
#[derive(Clone, Debug)]
pub struct Filter { pub blur: f32, pub saturate: f32, pub brightness: f32 }

fn filter(s: &str) -> Filter {
    let f = |name: &str, d: f32| s.find(&format!("{name}(")).and_then(|i| {
        let rest = &s[i + name.len() + 1..];
        rest[..rest.find(')')?].trim_end_matches("px").trim().parse().ok()
    }).unwrap_or(d);
    Filter { blur: f("blur", 0.0), saturate: f("saturate", 1.0), brightness: f("brightness", 1.0) }
}

#[derive(Clone, Debug)]
pub struct Look {
    pub name: String,
    pub light: bool,
    pub bg: Color, pub surface: Color, pub ink: Color, pub muted: Color, pub faint: Color,
    pub acc: Color, pub acc2: Color, pub err: Color, pub dim: Color, pub hi: Color,
    pub line: Color, pub bar: Color, pub barfg: Color, pub barb: Color, pub edge: Color, pub status: Color, pub off: Color,
    pub grain: f32, pub texture: String, pub scan: f32, pub glow: bool, pub misreg: bool, pub rules: bool,
    pub solid: Option<f32>,
    pub filter: Filter,
    pub rain: String,
    pub mono: Font, pub display: Font,
    pub transition: String, pub place: String,
}

pub struct Looks { pub all: Json }

impl Looks {
    pub fn load(path: &Path) -> Result<Looks, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let start = text.rfind("window.LOOKS =").ok_or("no window.LOOKS in looks.js")? + "window.LOOKS =".len();
        let end = text.rfind(';').ok_or("no ; in looks.js")?;
        Ok(Looks { all: Json::parse(&text[start..end])? })
    }

    fn font(&self, k: &str) -> Font {
        let f = self.all.get("fonts").get(k);
        let fam = f.get("family").str().unwrap_or("Consolas");
        // the first family in the CSS list, without its quotes: that's the bundled face
        let first = fam.split(',').next().unwrap_or(fam).trim().trim_matches('\'').trim_matches('"').to_string();
        Font { family: first, scale: f.get("scale").num().unwrap_or(1.0) as f32, weight: f.get("weight").num().unwrap_or(400.0) as u32 }
    }

    /// `want` is the feed's "look": {theme, palette, rain, font, transition}; "theme" or unknown means the theme's own
    pub fn resolve(&self, want: &Json) -> Look {
        let themes = self.all.get("themes");
        let pick = |k: &str| want.get(k).str().unwrap_or("").to_string();
        let mut name = pick("theme");
        if themes.get(&name).is_null() { name = "phosphor".into(); }
        let mut t = themes.get(&name).map();
        let pal = pick("palette");
        if pal != name && !themes.get(&pal).is_null() {   // borrow another theme's colors; fonts, background, texture stay
            let other = themes.get(&pal).map();
            for k in self.all.get("color_keys").items().iter().filter_map(|k| k.str()) {
                t.remove(k);
                if let Some(v) = other.get(k) { t.insert(k.to_string(), v.clone()); }
            }
        }
        let s = |k: &str| t.get(k).and_then(|v| v.str()).unwrap_or("").to_string();
        let c = |k: &str| color(&s(k)).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let opt = |k: &str| t.get(k).and_then(|v| v.str()).and_then(color);
        let n = |k: &str| t.get(k).and_then(|v| v.num()).map(|v| v as f32);
        let b = |k: &str| t.get(k).map(|v| v.truthy()).unwrap_or(false);
        let light = s("mode") == "light";
        let ink = c("ink");
        let mut rain = pick("rain");
        if self.all.get("backgrounds").get(&rain).is_null() { rain = s("rain"); }
        let chosen = pick("font");
        let chosen = if self.all.get("fonts").get(&chosen).is_null() { None } else { Some(chosen) };   // replaces both
        let mut transition = pick("transition");
        if self.all.get("transitions").get(&transition).is_null() { transition = s("transition"); }
        let place = self.all.get("transitions").get(&transition).get("place").str().unwrap_or("any").to_string();
        let fs = s("filter");
        Look {
            light, bg: c("bg"), surface: c("surface"), ink, muted: c("muted"), faint: c("faint"),
            acc: c("acc"), acc2: c("acc2"), err: c("err"), dim: c("dim"), hi: c("hi"),
            line: opt("line").unwrap_or(with_alpha(ink, if light { 0.22 } else { 0.3 })),
            bar: opt("bar").unwrap_or(with_alpha(ink, if light { 0.05 } else { 0.07 })),
            barfg: opt("barfg").unwrap_or(c("muted")), barb: opt("barb").unwrap_or(ink),
            edge: opt("edge").unwrap_or(with_alpha(ink, if light { 0.16 } else { 0.2 })),
            status: opt("status").unwrap_or(c("muted")), off: opt("off").unwrap_or(c("faint")),
            grain: n("grain").unwrap_or(0.0), texture: { let x = s("texture"); if x.is_empty() { "none".into() } else { x } },
            scan: n("scan").unwrap_or(0.42), glow: b("glow"), misreg: b("misreg"), rules: b("rules"),
            solid: n("solid"),
            filter: filter(if fs.is_empty() { if light { "blur(10px)" } else { "blur(14px) saturate(.5) brightness(.85)" } } else { &fs }),
            rain,
            mono: self.font(chosen.as_deref().unwrap_or(&s("mono"))),
            display: self.font(chosen.as_deref().unwrap_or(&s("display"))),
            transition, place, name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn looks() -> Looks { Looks::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/glitchwall/wallpaper/looks.js")).unwrap() }

    #[test]
    fn phosphor_by_default() {
        let l = looks().resolve(&Json::Null);
        assert_eq!(l.name, "phosphor");
        assert_eq!(l.rain, "pixel");
        assert_eq!(l.mono.family, "IBM Plex Mono");
        assert_eq!(l.display.family, "VT323");
        assert_eq!(l.transition, "crt");
        assert_eq!(l.texture, "tube");
        assert!(l.glow);
        assert_eq!(l.filter.blur, 14.0);
    }

    #[test]
    fn borrowed_palette_keeps_fonts_and_background() {
        let want = Json::parse(r#"{"theme":"newsprint","palette":"neon","rain":"theme","font":"theme","transition":"theme"}"#).unwrap();
        let l = looks().resolve(&want);
        assert_eq!(l.rain, "broadsheet");
        assert_eq!(l.mono.family, "DM Mono");
        assert_eq!(l.ink, color("#a8f0ff").unwrap());
        assert!(!l.light);
    }

    #[test]
    fn colors() {
        assert_eq!(color("rgba(255,0,0,.5)"), Some([1.0, 0.0, 0.0, 0.5]));
        assert_eq!(color("#000000"), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(color("none"), None);
    }
}
