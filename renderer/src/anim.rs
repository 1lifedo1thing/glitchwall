//! Card transitions, read from the page's own CSS @keyframes (index.html), so there is one definition of each.
//! Understood: opacity; transform translate/scale/skew/rotate; filter brightness/blur/saturate; clip-path inset().
//! Anything else in a keyframe (text-shadow RGB splits, hue-rotate, circle clips, CSS variables) is skipped.
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease { Linear, Bezier(f32, f32, f32, f32), Steps(u32) }

impl Ease {
    fn parse(s: &str) -> Ease {
        let s = s.trim();
        match s {
            "linear" => Ease::Linear,
            "ease" => Ease::Bezier(0.25, 0.1, 0.25, 1.0),
            "ease-in" => Ease::Bezier(0.42, 0.0, 1.0, 1.0),
            "ease-out" => Ease::Bezier(0.0, 0.0, 0.58, 1.0),
            "ease-in-out" => Ease::Bezier(0.42, 0.0, 0.58, 1.0),
            _ if s.starts_with("cubic-bezier(") => {
                let n: Vec<f32> = args(s).iter().filter_map(|a| a.parse().ok()).collect();
                if n.len() == 4 { Ease::Bezier(n[0], n[1], n[2], n[3]) } else { Ease::Linear }
            }
            _ if s.starts_with("steps(") => Ease::Steps(args(s).first().and_then(|a| a.parse().ok()).unwrap_or(1)),
            _ => Ease::Linear,
        }
    }
    pub fn at(self, t: f32) -> f32 {
        if t <= 0.0 { return 0.0; }
        if t >= 1.0 && !matches!(self, Ease::Steps(_)) { return 1.0; }
        match self {
            Ease::Linear => t,
            Ease::Steps(n) => ((t * n as f32).floor() / n as f32).min(1.0),   // steps(n) jumps at the end of each step
            Ease::Bezier(x1, y1, x2, y2) => {
                let b = |a: f32, c: f32, u: f32| 3.0 * a * u * (1.0 - u).powi(2) + 3.0 * c * u * u * (1.0 - u) + u.powi(3);
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                for _ in 0..24 { let m = (lo + hi) / 2.0; if b(x1, x2, m) < t { lo = m } else { hi = m } }
                b(y1, y2, (lo + hi) / 2.0)
            }
        }
    }
}

/// the state of a card at one moment of its transition
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub opacity: f32, pub tx: f32, pub ty: f32, pub sx: f32, pub sy: f32, pub skew: f32, pub rot: f32,
    pub bright: f32, pub blur: f32, pub sat: f32,
    pub clip: [f32; 4],   // inset top, right, bottom, left, in % of the card
}

pub const REST: Pose = Pose { opacity: 1.0, tx: 0.0, ty: 0.0, sx: 1.0, sy: 1.0, skew: 0.0, rot: 0.0, bright: 1.0, blur: 0.0, sat: 1.0, clip: [0.0; 4] };

const PROPS: usize = 13;
fn get(p: &Pose, i: usize) -> f32 {
    [p.opacity, p.tx, p.ty, p.sx, p.sy, p.skew, p.rot, p.bright, p.blur, p.sat, p.clip[0], p.clip[1], p.clip[2], p.clip[3]][i.min(13)]
}
fn set(p: &mut Pose, i: usize, v: f32) {
    match i { 0 => p.opacity = v, 1 => p.tx = v, 2 => p.ty = v, 3 => p.sx = v, 4 => p.sy = v, 5 => p.skew = v, 6 => p.rot = v,
              7 => p.bright = v, 8 => p.blur = v, 9 => p.sat = v, 10..=13 => p.clip[i - 10] = v, _ => {} }
}
// properties come in groups: a keyframe that sets transform sets all of the transform values, and so on
fn group(i: usize) -> usize { match i { 0 => 0, 1..=6 => 1, 7..=9 => 2, _ => 3 } }

/// one keyframe: where (0-1) and the groups it sets
#[derive(Clone, Debug)]
struct Frame { at: f32, pose: Pose, sets: [bool; 4] }

#[derive(Clone, Debug)]
pub struct Anim { frames: Vec<Frame>, pub dur: f32, ease: Ease }

impl Anim {
    /// the pose at `t` seconds in. CSS rules: each property runs between the keyframes that set it (0% and 100%
    /// default to the card at rest), and the timing function applies to each of those stretches on its own.
    pub fn pose(&self, t: f32) -> Pose {
        let u = (t / self.dur.max(1e-3)).clamp(0.0, 1.0);
        let mut out = REST;
        for i in 0..=PROPS {
            let g = group(i);
            let mut pts: Vec<(f32, f32)> = self.frames.iter().filter(|f| f.sets[g]).map(|f| (f.at, get(&f.pose, i))).collect();
            if pts.first().map(|p| p.0 > 0.0).unwrap_or(true) { pts.insert(0, (0.0, get(&REST, i))); }
            if pts.last().map(|p| p.0 < 1.0).unwrap_or(true) { pts.push((1.0, get(&REST, i))); }
            let k = pts.iter().rposition(|p| p.0 <= u).unwrap_or(0).min(pts.len() - 2);
            let (a, b) = (pts[k], pts[k + 1]);
            let local = if b.0 > a.0 { (u - a.0) / (b.0 - a.0) } else { 1.0 };
            set(&mut out, i, a.1 + (b.1 - a.1) * self.ease.at(local));
        }
        out
    }
}

/// every @keyframes block in the page, by name, as the raw text inside its braces
fn blocks(css: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = css;
    while let Some(i) = rest.find("@keyframes ") {
        rest = &rest[i + 11..];
        let Some(open) = rest.find('{') else { break };
        let name = rest[..open].trim().to_string();
        let (mut depth, mut end) = (0, None);
        for (j, ch) in rest[open..].char_indices() {
            if ch == '{' { depth += 1 } else if ch == '}' { depth -= 1; if depth == 0 { end = Some(open + j); break; } }
        }
        let Some(end) = end else { break };
        out.insert(name, rest[open + 1..end].to_string());
        rest = &rest[end..];
    }
    out
}

fn args(s: &str) -> Vec<String> {
    let (Some(a), Some(b)) = (s.find('('), s.rfind(')')) else { return vec![] };
    s[a + 1..b].split(|c| c == ',' || c == ' ').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()
}

/// "12px", "-3.5px", "18deg", ".5", "42%" -> the number
fn num(s: &str) -> Option<f32> {
    s.trim().trim_end_matches("px").trim_end_matches("deg").trim_end_matches('%').parse().ok()
}

/// the function calls in a CSS value: "scale(1,.01) translate(2px,0)" -> [("scale", ["1", ".01"]), ...]
fn calls(v: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let mut rest = v.trim();
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().to_string();
        let Some(close) = rest[open..].find(')') else { break };
        out.push((name, args(&rest[open..open + close + 1])));
        rest = &rest[open + close + 1..];
    }
    out
}

fn frame(sel: f32, decls: &str) -> Frame {
    let mut f = Frame { at: sel, pose: REST, sets: [false; 4] };
    for d in decls.split(';') {
        let Some((k, v)) = d.split_once(':') else { continue };
        let v = v.trim();
        match k.trim() {
            "opacity" => if let Some(n) = num(v) { f.pose.opacity = n; f.sets[0] = true; },
            "transform" => {
                f.sets[1] = true;
                for (name, a) in calls(v) {
                    let n: Vec<f32> = a.iter().map(|x| num(x).unwrap_or(0.0)).collect();
                    match (name.as_str(), n.len()) {
                        ("scale", 1) => { f.pose.sx = n[0]; f.pose.sy = n[0]; }
                        ("scale", 2) => { f.pose.sx = n[0]; f.pose.sy = n[1]; }
                        ("scaleX", 1) => f.pose.sx = n[0],
                        ("scaleY", 1) => f.pose.sy = n[0],
                        ("translate", 1) => f.pose.tx = n[0],
                        ("translate", 2) => { f.pose.tx = n[0]; f.pose.ty = n[1]; }
                        ("translateX", 1) => f.pose.tx = n[0],
                        ("translateY", 1) => f.pose.ty = n[0],
                        ("skewX", 1) | ("skew", _) => f.pose.skew = n[0],
                        ("rotate", 1) => f.pose.rot = n[0],
                        _ => {}
                    }
                }
            }
            "filter" => {
                f.sets[2] = true;
                for (name, a) in calls(v) {
                    let n = a.first().and_then(|x| num(x)).unwrap_or(0.0);
                    match name.as_str() { "brightness" => f.pose.bright = n, "blur" => f.pose.blur = n, "saturate" => f.pose.sat = n, _ => {} }
                }
            }
            "clip-path" => {
                if let Some((_, a)) = calls(v).into_iter().find(|(n, _)| n == "inset") {
                    let n: Vec<f32> = a.iter().map(|x| num(x).unwrap_or(0.0)).collect();
                    f.pose.clip = match n.len() { 1 => [n[0]; 4], 2 => [n[0], n[1], n[0], n[1]], 3 => [n[0], n[1], n[2], n[1]],
                                                  4 => [n[0], n[1], n[2], n[3]], _ => [0.0; 4] };
                    f.sets[3] = true;
                }
            }
            _ => {}
        }
    }
    f
}

/// a transition's in or out half, from its looks.js spec: "crtIn .5s cubic-bezier(.2,.7,.3,1) both"
pub fn parse(css: &str, spec: &str) -> Option<Anim> {
    let name = spec.split_whitespace().next()?;
    let body = blocks(css).remove(name)?;
    let rest = spec[name.len()..].trim();
    let dur = rest.split_whitespace().find_map(|w| w.strip_suffix("ms").and_then(|n| n.parse::<f32>().ok()).map(|n| n / 1000.0)
                                                   .or_else(|| w.strip_suffix('s').and_then(|n| n.parse().ok()))).unwrap_or(0.5);
    let ease_txt = rest.split_whitespace().skip(1).filter(|w| !matches!(*w, "both" | "forwards" | "backwards" | "none")).collect::<Vec<_>>().join(" ");
    let mut frames = Vec::new();
    let mut r = body.as_str();
    while let Some(open) = r.find('{') {
        let sel = &r[..open];
        let Some(close) = r[open..].find('}') else { break };
        let decls = &r[open + 1..open + close];
        for s in sel.split(',') {
            let s = s.trim();
            let at = match s { "from" => Some(0.0), "to" => Some(100.0), _ => num(s) };
            if let Some(at) = at { frames.push(frame(at / 100.0, decls)); }
        }
        r = &r[open + close + 1..];
    }
    frames.sort_by(|a, b| a.at.total_cmp(&b.at));
    Some(Anim { frames, dur, ease: if ease_txt.is_empty() { Ease::Bezier(0.25, 0.1, 0.25, 1.0) } else { Ease::parse(&ease_txt) } })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page() -> String { std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/glitchwall/wallpaper/index.html")).unwrap() }

    #[test]
    fn crt_opens_from_a_line() {
        let a = parse(&page(), "crtIn .5s cubic-bezier(.2,.7,.3,1) both").unwrap();
        assert_eq!(a.dur, 0.5);
        let p = a.pose(0.0);
        assert_eq!((p.opacity, p.sx, p.sy, p.bright), (0.0, 0.5, 0.01, 3.0));
        assert_eq!(a.pose(0.5), REST);
        let mid = a.pose(0.3);     // between 35% (a line) and 70% (slightly over full height)
        assert!(mid.sy > 0.01 && mid.sy < 1.04 && mid.opacity == 1.0);
    }

    #[test]
    fn every_transition_parses() {
        let css = page();
        let looks = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/glitchwall/wallpaper/looks.js")).unwrap();
        let start = looks.rfind("window.LOOKS =").unwrap() + 14;
        let j = crate::json::Json::parse(&looks[start..looks.rfind(';').unwrap()]).unwrap();
        for (name, t) in j.get("transitions").entries() {
            for k in ["in", "out"] {
                let a = parse(&css, t.get(k).str().unwrap()).unwrap_or_else(|| panic!("{name}.{k}"));
                assert!(a.dur > 0.0 && a.dur <= 1.0, "{name}.{k}");
                let end = a.pose(a.dur);
                if k == "in" { assert!((end.opacity - 1.0).abs() < 1e-3, "{name} ends visible"); }
            }
        }
    }

    #[test]
    fn steps_and_bezier() {
        assert_eq!(Ease::Steps(4).at(0.3), 0.25);
        assert!((Ease::parse("ease-in").at(0.5) - 0.315).abs() < 0.01);
    }
}
