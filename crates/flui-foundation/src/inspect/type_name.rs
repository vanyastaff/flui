//! A kind and its type arguments read from a [`core::any::type_name`] spelling, for trees
//! whose nodes are erased (`dyn View`, `dyn RenderObject`) and know only their type's name.

/// The kind `type_name` spells: its last path segment, without type arguments.
///
/// `flui_widgets::basic::Padding` is `Padding`, `a::Builder<b::C>` is `Builder`, and a name
/// with no path is itself.
#[must_use]
pub fn kind_of(type_name: &'static str) -> &'static str {
    let head = type_name.find('<').map_or(type_name, |at| &type_name[..at]);
    head.rsplit("::").next().unwrap_or(head)
}

/// Calls `args` with each type argument `type_name` spells, in order, as written.
///
/// `a::Builder<b::C, d::E<f::G>>` yields `b::C` and `d::E<f::G>`; a name with no type
/// arguments yields nothing.
pub fn kind_args_of(type_name: &'static str, args: &mut dyn FnMut(&'static str)) {
    let Some(open) = type_name.find('<') else {
        return;
    };
    let Some(inner) = type_name[open + 1..].strip_suffix('>') else {
        return;
    };
    let mut depth = 0_u32;
    let mut start = 0;
    let mut previous = '\0';
    for (at, ch) in inner.char_indices() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            // The `>` of a function type's `->` closes nothing.
            '>' if previous == '-' => {}
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args(inner[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
        previous = ch;
    }
    args(inner[start..].trim());
}
