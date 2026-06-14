//! Painted-command introspection IR + stable text formatting; the fmt helpers
//! are the single source of the normalized text format — 2-decimal floats,
//! #RRGGBBAA, identity-transform omitted — shared by the scene-snapshot IR's
//! Display impls.

/// Stable text-formatting primitives for the scene-snapshot normalized format.
///
/// Every helper in this module is a single source of truth for one formatting
/// rule. Consumers (the snapshot serializer in `flui-rendering`, and the IR
/// `Display` impls added in later tasks) import from here so the format cannot
/// drift between producers.
///
/// Stability contract: once the line format is chosen (floats 2-dec, color
/// `#RRGGBBAA`, transform omitted unless non-identity) it must not change
/// without a coordinated golden update.
#[doc(hidden)]
pub mod fmt {
    use flui_types::{
        geometry::{Matrix4, Pixels, Point, RRect, Rect},
        painting::Clip,
        styling::Color,
    };

    use crate::PaintStyle;
    use crate::display_list::{ClipOp, Paint};

    /// Format one `f32` to 2 decimal places, normalizing `-0.0` → `0.0`.
    ///
    /// # Panics (debug only)
    ///
    /// Triggers a `debug_assert!` if `v` is not finite. A non-finite value
    /// would format as `"NaN"` or `"inf"` and break the fixed-decimal snapshot
    /// invariant — it signals a bug in the render object that produced the
    /// command, not here.
    pub fn f(v: f32) -> String {
        // Stability contract: callers pass finite floats. A non-finite value would
        // format as "NaN"/"inf" and break the fixed-decimal snapshot invariant — it
        // signals a bug in the render object that produced the command, not here.
        debug_assert!(
            v.is_finite(),
            "snapshot: non-finite float in a draw command"
        );
        // Normalize negative zero before formatting.
        let v = if v == 0.0 { 0.0_f32 } else { v };
        format!("{v:.2}")
    }

    /// Format a `Color` as `#RRGGBBAA`.
    pub fn hex_color(c: Color) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
    }

    /// Summarize a `Paint` as `"<style> <#RRGGBBAA>[ stroke=<w>]"`.
    pub fn summarize_paint(paint: &Paint) -> String {
        let style = match paint.style {
            PaintStyle::Fill => "fill",
            PaintStyle::Stroke => "stroke",
        };
        let color = hex_color(paint.color);
        if matches!(paint.style, PaintStyle::Stroke) {
            format!("{style} {color} stroke={}", f(paint.stroke_width))
        } else {
            format!("{style} {color}")
        }
    }

    /// Format a `Rect<Pixels>` as `"(l,t WxH)"`.
    pub fn fmt_rect(r: Rect<Pixels>) -> String {
        format!(
            "({},{} {}x{})",
            f(r.left().get()),
            f(r.top().get()),
            f(r.width().get()),
            f(r.height().get()),
        )
    }

    /// Format a `Point<Pixels>` as `"(x,y)"`.
    pub fn fmt_point(p: Point<Pixels>) -> String {
        format!("({},{})", f(p.x.get()), f(p.y.get()))
    }

    /// Format an `RRect` as `"(l,t WxH r=tl/tr/br/bl)"`.
    ///
    /// Uses the `rect` field of `RRect` for geometry and the four corner radii
    /// (circular approximation: `x` component of each radius).
    pub fn fmt_rrect(rr: &RRect) -> String {
        let r = rr.rect;
        format!(
            "({},{} {}x{} r={}/{}/{}/{})",
            f(r.left().get()),
            f(r.top().get()),
            f(r.width().get()),
            f(r.height().get()),
            f(rr.top_left.x.get()),
            f(rr.top_right.x.get()),
            f(rr.bottom_right.x.get()),
            f(rr.bottom_left.x.get()),
        )
    }

    /// Format a `ClipOp` as a short lowercase string.
    pub fn fmt_clip_op(op: ClipOp) -> &'static str {
        match op {
            ClipOp::Intersect => "intersect",
            ClipOp::Difference => "difference",
        }
    }

    /// Format a `Clip` behavior as a short lowercase string.
    ///
    /// Distinct rendering qualities must serialize distinctly so a regression
    /// that swaps, say, `AntiAlias` for `HardEdge` shows up as a snapshot diff
    /// instead of passing silently.
    pub fn fmt_clip(behavior: Clip) -> &'static str {
        match behavior {
            Clip::None => "none",
            Clip::HardEdge => "hard",
            Clip::AntiAlias => "antialias",
            Clip::AntiAliasWithSaveLayer => "antialias-savelayer",
        }
    }

    /// Append a transform suffix when the matrix is non-identity.
    pub fn maybe_transform(transform: &Matrix4) -> String {
        if transform.is_identity() {
            return String::new();
        }
        // Build the bracket inline without an intermediate `Vec` allocation.
        let mut s = " xf=[".to_owned();
        let mut first = true;
        for v in &transform.m {
            if !first {
                s.push(',');
            }
            first = false;
            s.push_str(&f(*v));
        }
        s.push(']');
        s
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use flui_types::styling::Color;

    use super::fmt::{f, hex_color};

    #[test]
    fn float_two_decimals_normalizes_neg_zero() {
        assert_eq!(f(-0.0), "0.00");
        assert_eq!(f(1.5), "1.50");
    }

    #[test]
    fn color_uppercase_rrggbbaa() {
        assert_eq!(hex_color(Color::rgba(255, 0, 0, 255)), "#FF0000FF");
    }
}
