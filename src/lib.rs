//! Whisker Drawapp — a simple finger-painting canvas.
//!
//! Touch points are collected via the touch handlers on a full-size
//! `view`, then rendered as SVG `<polyline>`s through `whisker-svg`.
//! The canvas is measured on mount so touch coordinates (LynxView space)
//! line up 1:1 with the SVG viewBox.

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

/// A completed stroke: its color and the points (in canvas-local px).
#[derive(Clone)]
struct Stroke {
    color: String,
    points: Vec<(f64, f64)>,
}

/// Render one stroke's points into the SVG body.
fn push_stroke(body: &mut String, color: &str, points: &[(f64, f64)]) {
    match points {
        [] => {}
        // A single point (a tap) draws as a dot.
        [(x, y)] => {
            body.push_str(&format!(
                "<circle cx=\"{x}\" cy=\"{y}\" r=\"2\" fill=\"{color}\"/>"
            ));
        }
        pts => {
            let coords = pts
                .iter()
                .map(|(x, y)| format!("{x},{y}"))
                .collect::<Vec<_>>()
                .join(" ");
            body.push_str(&format!(
                "<polyline points=\"{coords}\" fill=\"none\" stroke=\"{color}\" \
                 stroke-width=\"3\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>"
            ));
        }
    }
}

/// Build the full SVG document for the current drawing state.
fn build_svg(w: f64, h: f64, strokes: &[Stroke], current: &[(f64, f64)], current_color: &str) -> String {
    if w <= 0.0 || h <= 0.0 {
        return String::new();
    }
    let mut body = String::new();
    for s in strokes {
        push_stroke(&mut body, &s.color, &s.points);
    }
    push_stroke(&mut body, current_color, current);
    format!("<svg viewBox=\"0 0 {w} {h}\">{body}</svg>")
}

#[whisker::main]
fn app() -> Element {
    // Completed strokes + the one currently being drawn.
    let strokes = RwSignal::new(Vec::<Stroke>::new());
    let current = RwSignal::new(Vec::<(f64, f64)>::new());
    // Selected color (defaults to the first palette entry).
    let color = RwSignal::new(COLORS[0].to_string());
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

    // The SVG recompiles whenever strokes / current / color change.
    let svg_content = computed(move || {
        let (_, _, w, h) = geom.get();
        let cur = current.get();
        let strokes = strokes.get();
        build_svg(w, h, &strokes, &cur, &color.get())
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

            // ── Drawing canvas ─────────────────────────────────────
            view(
                ref: canvas.r(),
                style: "flex: 1; background-color: #ffffff;",
                on_touchstart: move |e| {
                    let (left, top, _, _) = geom.get();
                    current.set(vec![(e.detail.x - left, e.detail.y - top)]);
                },
                on_touchmove: move |e| {
                    let (left, top, _, _) = geom.get();
                    current.update(|v| v.push((e.detail.x - left, e.detail.y - top)));
                },
                on_touchend: move |_| {
                    let pts = current.get();
                    if !pts.is_empty() {
                        let c = color.get();
                        strokes.update(|s| s.push(Stroke { color: c, points: pts }));
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
