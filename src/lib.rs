//! Whisker Drawapp — a finger-painting canvas with selectable pens.
//!
//! Touch points are collected via the touch handlers on a full-size
//! `view`, then rendered as SVG shapes through `whisker-svg`. The
//! canvas is measured on mount so touch coordinates (LynxView space)
//! line up 1:1 with the SVG viewBox.
//!
//! ## Why everything is geometry
//!
//! The `whisker-svg` v1 compiler (see `packages/whisker-svg/SPEC.md`)
//! only understands `fill`, `stroke`, `stroke-width` and group
//! `opacity`. There is deliberately **no** `stroke-dasharray`,
//! `stroke-opacity`, `stroke-linecap` or `stroke-linejoin` — strokes
//! are always butt-capped + miter-joined (SPEC `0x32 PATH_STROKE`).
//!
//! So each pen is built out of primitives the compiler does support:
//!   * round caps / joins  → `<circle>`s at the path vertices
//!   * translucency        → a `<g opacity="…">` wrapper (group
//!                            opacity composites once, so overlapping
//!                            sub-shapes don't darken at the seams)
//!   * dashes / dots       → the polyline resampled by arc length
//!                            into many short shapes
//!   * calligraphy         → a variable-width filled polygon (`fill`)

use std::fmt::Write as _;

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_svg::Svg;

/// Palette shown in the top bar. First entry is the default stroke color.
const COLORS: [&str; 6] = [
    "#111111", // ink
    "#ef4444", // red
    "#f59e0b", // amber
    "#22c55e", // green
    "#3b82f6", // blue
    "#a855f7", // purple
];

/// The kinds of pen the user can draw with. Each kind fully determines
/// how a stroke is rendered (width, caps, dashes, layering, …); a
/// `Stroke` stores its kind so switching pens never alters strokes that
/// were already drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PenKind {
    /// Thin solid line, round cap. (ballpoint)
    Ballpoint,
    /// Medium solid line, round cap. (marker)
    Marker,
    /// Thick solid line, round cap + round join. (brush)
    Brush,
    /// Thick, semi-transparent, square cap; drawn under the ink layer.
    Highlighter,
    /// Dashed line (geometry dashes, since dasharray is unsupported).
    Dashed,
    /// Dotted line: round dots spaced along the path.
    Dotted,
    /// Optional pen #9 — glow approximated by stacking a wide
    /// translucent halo under a thin bright core.
    Neon,
    /// Optional pen #8 — speed-driven thick/thin filled ribbon. The
    /// simulator has no pressure, so the local point spacing stands in
    /// for stroke speed (slow = fat, fast = thin).
    Calligraphy,
    /// Not an ink at all: a *mode* that deletes strokes near the touch.
    Eraser,
}

/// Display order of the pen selector. Eraser sits last.
const PENS: [PenKind; 9] = [
    PenKind::Ballpoint,
    PenKind::Marker,
    PenKind::Brush,
    PenKind::Highlighter,
    PenKind::Dashed,
    PenKind::Dotted,
    PenKind::Neon,
    PenKind::Calligraphy,
    PenKind::Eraser,
];

impl PenKind {
    /// Stable key for `ForEach` / selection comparisons.
    fn id(self) -> &'static str {
        match self {
            PenKind::Ballpoint => "ball",
            PenKind::Marker => "marker",
            PenKind::Brush => "brush",
            PenKind::Highlighter => "hl",
            PenKind::Dashed => "dash",
            PenKind::Dotted => "dot",
            PenKind::Neon => "neon",
            PenKind::Calligraphy => "calli",
            PenKind::Eraser => "erase",
        }
    }

    /// Short label shown on the selector button.
    fn label(self) -> &'static str {
        match self {
            PenKind::Ballpoint => "Pen",
            PenKind::Marker => "Marker",
            PenKind::Brush => "Brush",
            PenKind::Highlighter => "HiLite",
            PenKind::Dashed => "Dash",
            PenKind::Dotted => "Dot",
            PenKind::Neon => "Neon",
            PenKind::Calligraphy => "Calli",
            PenKind::Eraser => "Erase",
        }
    }

    /// Default stroke width (in canvas px / SVG user units) for the kind.
    fn default_width(self) -> f64 {
        match self {
            PenKind::Ballpoint => 2.5,
            PenKind::Marker => 7.0,
            PenKind::Brush => 16.0,
            PenKind::Highlighter => 22.0,
            PenKind::Dashed => 4.0,
            PenKind::Dotted => 6.0,
            PenKind::Neon => 5.0,
            PenKind::Calligraphy => 11.0,
            PenKind::Eraser => 18.0,
        }
    }

    /// Highlighter draws *under* every other stroke so it reads as a
    /// background wash rather than covering the ink.
    fn is_underlay(self) -> bool {
        matches!(self, PenKind::Highlighter)
    }
}

/// A completed stroke. Carries everything needed to re-render it
/// independently of the currently selected pen.
#[derive(Clone)]
struct Stroke {
    kind: PenKind,
    color: String,
    width: f64,
    points: Vec<(f64, f64)>,
}

// ── geometry helpers ───────────────────────────────────────────────

/// Format a coordinate compactly (2 dp keeps the display list small).
fn n(v: f64) -> String {
    format!("{v:.2}")
}

/// `"x,y x,y …"` for a `<polyline>`/`<polygon>` `points` attribute.
fn poly_points(pts: &[(f64, f64)]) -> String {
    let mut s = String::with_capacity(pts.len() * 12);
    for (i, (x, y)) in pts.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{},{}", n(*x), n(*y));
    }
    s
}

/// Squared distance from point `p` to segment `a`–`b`.
fn dist2_point_seg(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (px, py) = p;
    let (ax, ay) = a;
    let (bx, by) = b;
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    if len2 <= f64::EPSILON {
        let (ex, ey) = (px - ax, py - ay);
        return ex * ex + ey * ey;
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    let (ex, ey) = (px - cx, py - cy);
    ex * ex + ey * ey
}

/// True if the eraser at `p` (with half-size `reach`) touches `s`.
fn stroke_hit(s: &Stroke, p: (f64, f64), reach: f64) -> bool {
    let r = reach + s.width / 2.0;
    let r2 = r * r;
    match s.points.as_slice() {
        [] => false,
        [a] => {
            let (dx, dy) = (p.0 - a.0, p.1 - a.1);
            dx * dx + dy * dy <= r2
        }
        pts => pts.windows(2).any(|w| dist2_point_seg(p, w[0], w[1]) <= r2),
    }
}

/// Reach (half-size) of the eraser hit test, in canvas px.
const ERASER_REACH: f64 = 11.0;

/// Split a polyline into the "on" runs of a dash pattern by walking it
/// at constant arc length. Returns each pen-down run as its own point
/// list. (We can't use `stroke-dasharray`, so we materialise the dashes.)
fn dash_runs(pts: &[(f64, f64)], on: f64, off: f64) -> Vec<Vec<(f64, f64)>> {
    let mut runs = Vec::new();
    if pts.len() < 2 {
        return runs;
    }
    let mut cur: Vec<(f64, f64)> = vec![pts[0]];
    let mut drawing = true;
    let mut remain = on;
    for w in pts.windows(2) {
        let (mut ax, mut ay) = w[0];
        let (bx, by) = w[1];
        let mut seg = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
        let d = seg.max(1e-9);
        let (dirx, diry) = ((bx - ax) / d, (by - ay) / d);
        while seg > remain {
            ax += dirx * remain;
            ay += diry * remain;
            seg -= remain;
            if drawing {
                cur.push((ax, ay));
                runs.push(std::mem::take(&mut cur));
                drawing = false;
                remain = off;
            } else {
                drawing = true;
                remain = on;
                cur.push((ax, ay));
            }
        }
        remain -= seg;
        if drawing {
            cur.push((bx, by));
        }
    }
    if drawing && cur.len() >= 2 {
        runs.push(cur);
    }
    runs
}

/// Positions of dots placed every `spacing` units along the polyline.
fn dot_positions(pts: &[(f64, f64)], spacing: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    if pts.is_empty() {
        return out;
    }
    out.push(pts[0]);
    let mut to_next = spacing;
    for w in pts.windows(2) {
        let (mut ax, mut ay) = w[0];
        let (bx, by) = w[1];
        let mut seg = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
        let d = seg.max(1e-9);
        let (dirx, diry) = ((bx - ax) / d, (by - ay) / d);
        while seg >= to_next {
            ax += dirx * to_next;
            ay += diry * to_next;
            seg -= to_next;
            out.push((ax, ay));
            to_next = spacing;
        }
        to_next -= seg;
    }
    out
}

// ── per-pen rendering ──────────────────────────────────────────────

/// Append a filled circle (used for dots and round caps/joins).
fn push_dot(body: &mut String, x: f64, y: f64, r: f64, fill: &str) {
    let _ = write!(
        body,
        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>",
        n(x),
        n(y),
        n(r),
        fill
    );
}

/// A plain solid polyline. `round` adds vertex dots to fake round
/// caps + joins (the compiler only does butt caps / miter joins).
fn solid_poly(body: &mut String, s: &Stroke, round: bool) {
    match s.points.as_slice() {
        [] => {}
        [(x, y)] => push_dot(body, *x, *y, s.width / 2.0, &s.color),
        pts => {
            let _ = write!(
                body,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/>",
                poly_points(pts),
                s.color,
                n(s.width)
            );
            if round {
                for (x, y) in pts {
                    push_dot(body, *x, *y, s.width / 2.0, &s.color);
                }
            }
        }
    }
}

/// Highlighter: a wide square-capped stroke, wrapped in a group so its
/// translucency composites once (no darkened overlaps). Underlaid below
/// the ink in `build_svg`.
fn highlighter(body: &mut String, s: &Stroke) {
    body.push_str("<g opacity=\"0.4\">");
    match s.points.as_slice() {
        [] => {}
        [(x, y)] => push_dot(body, *x, *y, s.width / 2.0, &s.color),
        pts => {
            let _ = write!(
                body,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/>",
                poly_points(pts),
                s.color,
                n(s.width)
            );
        }
    }
    body.push_str("</g>");
}

/// Dashed line: each pen-down run becomes its own polyline.
fn dashed(body: &mut String, s: &Stroke) {
    match s.points.as_slice() {
        [] => {}
        [(x, y)] => push_dot(body, *x, *y, s.width / 2.0, &s.color),
        pts => {
            for run in dash_runs(pts, s.width * 3.5, s.width * 2.5) {
                let _ = write!(
                    body,
                    "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/>",
                    poly_points(&run),
                    s.color,
                    n(s.width)
                );
            }
        }
    }
}

/// Dotted line: round dots spaced along the path.
fn dotted(body: &mut String, s: &Stroke) {
    for (x, y) in dot_positions(&s.points, s.width * 2.2) {
        push_dot(body, x, y, s.width / 2.0, &s.color);
    }
}

/// Neon: a wide translucent halo (grouped, so it doesn't self-darken)
/// under the coloured line, topped by a thin near-white core.
fn neon(body: &mut String, s: &Stroke) {
    match s.points.as_slice() {
        [] => {}
        [(x, y)] => {
            body.push_str("<g opacity=\"0.5\">");
            push_dot(body, *x, *y, s.width * 1.2, &s.color);
            body.push_str("</g>");
            push_dot(body, *x, *y, s.width / 2.0, &s.color);
            push_dot(body, *x, *y, s.width * 0.2, "#ffffff");
        }
        pts => {
            let line = poly_points(pts);
            let _ = write!(
                body,
                "<g opacity=\"0.5\"><polyline points=\"{}\" fill=\"none\" stroke=\"{}\" \
                 stroke-width=\"{}\"/></g>",
                line,
                s.color,
                n(s.width * 2.4)
            );
            let _ = write!(
                body,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/>",
                line,
                s.color,
                n(s.width)
            );
            let _ = write!(
                body,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"{}\"/>",
                line,
                n(s.width * 0.4)
            );
        }
    }
}

/// Calligraphy: build a filled ribbon whose half-width varies with the
/// local point spacing (our stand-in for pen speed: slow = fat).
fn calligraphy(body: &mut String, s: &Stroke) {
    let pts = &s.points;
    if pts.len() < 2 {
        if let Some((x, y)) = pts.first() {
            push_dot(body, *x, *y, s.width / 2.0, &s.color);
        }
        return;
    }

    let base = s.width;
    let nlen = pts.len();
    // Per-point half-width from local speed (avg neighbour spacing).
    let mut hw = vec![0.0_f64; nlen];
    for i in 0..nlen {
        let prev = pts[i.saturating_sub(1)];
        let next = pts[(i + 1).min(nlen - 1)];
        let speed = ((next.0 - prev.0).powi(2) + (next.1 - prev.1).powi(2)).sqrt() / 2.0;
        let t = (speed / 26.0).clamp(0.0, 1.0);
        // slow (t→0): ~0.7*base ; fast (t→1): ~0.15*base
        hw[i] = base * 0.5 * (1.4 - 1.1 * t);
    }

    // Offset the centreline by ±half-width along the local normal to
    // make the two sides of the ribbon, then close into one polygon.
    let mut left = Vec::with_capacity(nlen);
    let mut right = Vec::with_capacity(nlen);
    for i in 0..nlen {
        let prev = pts[i.saturating_sub(1)];
        let next = pts[(i + 1).min(nlen - 1)];
        let (tx, ty) = (next.0 - prev.0, next.1 - prev.1);
        let len = (tx * tx + ty * ty).sqrt().max(1e-9);
        // normal = tangent rotated 90°
        let (nx, ny) = (-ty / len, tx / len);
        let (px, py) = pts[i];
        left.push((px + nx * hw[i], py + ny * hw[i]));
        right.push((px - nx * hw[i], py - ny * hw[i]));
    }
    let mut ring = left;
    ring.extend(right.into_iter().rev());

    let _ = write!(
        body,
        "<polygon points=\"{}\" fill=\"{}\"/>",
        poly_points(&ring),
        s.color
    );
}

/// Dispatch a stroke to its pen renderer.
fn render_stroke(body: &mut String, s: &Stroke) {
    match s.kind {
        PenKind::Ballpoint | PenKind::Marker => solid_poly(body, s, false),
        PenKind::Brush => solid_poly(body, s, true),
        PenKind::Highlighter => highlighter(body, s),
        PenKind::Dashed => dashed(body, s),
        PenKind::Dotted => dotted(body, s),
        PenKind::Neon => neon(body, s),
        PenKind::Calligraphy => calligraphy(body, s),
        // The eraser is a mode — it never produces a stored stroke.
        PenKind::Eraser => {}
    }
}

/// Build the full SVG document for the current drawing state. Highlighter
/// strokes are emitted first (underlay); everything else paints on top.
/// Within each layer, draw order is preserved.
fn build_svg(w: f64, h: f64, strokes: &[Stroke], current: Option<&Stroke>) -> String {
    if w <= 0.0 || h <= 0.0 {
        return String::new();
    }
    let mut body = String::new();
    for s in strokes.iter().filter(|s| s.kind.is_underlay()) {
        render_stroke(&mut body, s);
    }
    if let Some(c) = current.filter(|c| c.kind.is_underlay()) {
        render_stroke(&mut body, c);
    }
    for s in strokes.iter().filter(|s| !s.kind.is_underlay()) {
        render_stroke(&mut body, s);
    }
    if let Some(c) = current.filter(|c| !c.kind.is_underlay()) {
        render_stroke(&mut body, c);
    }
    format!("<svg viewBox=\"0 0 {w} {h}\">{body}</svg>")
}

#[whisker::main]
fn app() -> Element {
    // Completed strokes + the one currently being drawn.
    let strokes = RwSignal::new(Vec::<Stroke>::new());
    let current = RwSignal::new(Vec::<(f64, f64)>::new());
    // Selected color + pen (default to the first palette entry / ballpoint).
    let color = RwSignal::new(COLORS[0].to_string());
    let pen = RwSignal::new(PenKind::Ballpoint);
    // Canvas geometry in LynxView coords: (left, top, width, height).
    let geom = RwSignal::new((0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64));

    // Measure the canvas once it mounts so touch coords map to the viewBox.
    let canvas = ElementHandle::new();
    on_mount(move || {
        spawn_local(async move {
            if let Ok(r) = canvas.bounding_client_rect().await {
                geom.set((r.left, r.top, r.width, r.height));
            }
        });
    });

    // The SVG recompiles whenever strokes / current / color / pen change.
    let svg_content = computed(move || {
        let (_, _, w, h) = geom.get();
        let cur = current.get();
        let k = pen.get();
        // The in-progress stroke previews with the live pen settings.
        // The eraser draws nothing — it just deletes on contact.
        let preview = if cur.is_empty() || k == PenKind::Eraser {
            None
        } else {
            Some(Stroke {
                kind: k,
                color: color.get(),
                width: k.default_width(),
                points: cur,
            })
        };
        let strokes = strokes.get();
        build_svg(w, h, &strokes, preview.as_ref())
    });

    render! {
        page(style: "flex-direction: column; background-color: #18181b;") {
            // ── Top bar: color palette + Undo / Clear ──────────────
            view(style: "flex-direction: row; align-items: center; gap: 12px; \
                         padding: 14px 16px; background-color: #27272a;") {
                ForEach(
                    each: move || COLORS.to_vec(),
                    key: |c: &&str| c.to_string(),
                    children: move |c: &'static str| {
                        let swatch_style = computed(move || {
                            let border = if color.get() == c { "#ffffff" } else { "#52525b" };
                            format!(
                                "width: 30px; height: 30px; border-radius: 15px; \
                                 background-color: {c}; border-width: 3px; \
                                 border-style: solid; border-color: {border};"
                            )
                        });
                        render! {
                            view(style: swatch_style, on_tap: move |_| color.set(c.to_string())) {}
                        }
                    },
                )

                // Spacer pushes the action buttons to the right.
                view(style: "flex: 1;") {}

                view(
                    style: "padding: 8px 14px; border-radius: 8px; background-color: #3f3f46;",
                    on_tap: move |_| { strokes.update(|s| { s.pop(); }); },
                ) {
                    text(value: "Undo", style: "color: #fafafa; font-size: 14px;")
                }
                view(
                    style: "padding: 8px 14px; border-radius: 8px; background-color: #3f3f46;",
                    on_tap: move |_| { strokes.set(Vec::new()); current.set(Vec::new()); },
                ) {
                    text(value: "Clear", style: "color: #fafafa; font-size: 14px;")
                }
            }

            // ── Pen selector ───────────────────────────────────────
            view(style: "flex-direction: row; align-items: center; flex-wrap: wrap; \
                         gap: 8px; padding: 10px 16px; background-color: #1f1f23;") {
                ForEach(
                    each: move || PENS.to_vec(),
                    key: |p: &PenKind| p.id().to_string(),
                    children: move |p: PenKind| {
                        let btn_style = computed(move || {
                            let selected = pen.get() == p;
                            let (bg, border) = if selected {
                                ("#4f46e5", "#c7d2fe")
                            } else {
                                ("#3f3f46", "#3f3f46")
                            };
                            format!(
                                "padding: 7px 12px; border-radius: 8px; background-color: {bg}; \
                                 border-width: 2px; border-style: solid; border-color: {border};"
                            )
                        });
                        render! {
                            view(style: btn_style, on_tap: move |_| pen.set(p)) {
                                text(value: p.label(), style: "color: #fafafa; font-size: 13px;")
                            }
                        }
                    },
                )
            }

            // ── Drawing canvas ─────────────────────────────────────
            view(
                ref: canvas.r(),
                style: "flex: 1; background-color: #ffffff;",
                on_touchstart: move |e| {
                    let (left, top, _, _) = geom.get();
                    let pt = (e.detail.x - left, e.detail.y - top);
                    if pen.get() == PenKind::Eraser {
                        strokes.update(|v| v.retain(|s| !stroke_hit(s, pt, ERASER_REACH)));
                    } else {
                        current.set(vec![pt]);
                    }
                },
                on_touchmove: move |e| {
                    let (left, top, _, _) = geom.get();
                    let pt = (e.detail.x - left, e.detail.y - top);
                    if pen.get() == PenKind::Eraser {
                        strokes.update(|v| v.retain(|s| !stroke_hit(s, pt, ERASER_REACH)));
                    } else {
                        current.update(|v| v.push(pt));
                    }
                },
                on_touchend: move |_| {
                    let k = pen.get();
                    if k != PenKind::Eraser {
                        let pts = current.get();
                        if !pts.is_empty() {
                            strokes.update(|s| s.push(Stroke {
                                kind: k,
                                color: color.get(),
                                width: k.default_width(),
                                points: pts,
                            }));
                        }
                    }
                    current.set(Vec::new());
                },
            ) {
                Svg(
                    content: svg_content,
                    color: "#000000",
                    style: "width: 100%; height: 100%;",
                )
            }
        }
    }
}
