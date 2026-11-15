//! The elements the web host puts in the page for one canvas window, in the
//! order it keeps them (ADR-0255 decision 16): the canvas, the text-input
//! host (`flui-text-input`), which holds the hidden editor while one serves a
//! field, and the semantics host (`flui-semantics`), which holds the
//! accessibility placeholder and then the ARIA DOM. The semantics host is
//! last, so nothing the host adds after it can stand between a platform
//! view and the pointer.
//!
//! A page with no canvas of its own gets a root, `flui-view`, fixed over the
//! viewport with the three inside it. A page that supplies a `<canvas
//! id="flui-canvas">` owns its layout: the two hosts follow the canvas as its
//! siblings, and the canvas gets back the `style` and `tabindex` the window
//! wrote once the window retires. The hosts are `display: contents`, so they
//! give their children no box, ancestor or containing block the canvas does
//! not have.
//!
//! Dropping a [`PageRoot`] takes out of the page what it put there, on every
//! exit: a window that closes, and one that fails to open after its root was
//! made.

use flui_foundation::geometry::EdgeInsets;
use wasm_bindgen::{JsCast as _, JsValue};
use web_sys::{Document, HtmlCanvasElement, HtmlElement};

use crate::owner::web::HostSlots;

/// The id a page gives its own canvas.
const PAGE_CANVAS_ID: &str = "flui-canvas";
/// The root the host makes when the page has no canvas of its own.
const ROOT_TAG: &str = "flui-view";
const TEXT_INPUT_TAG: &str = "flui-text-input";
const SEMANTICS_TAG: &str = "flui-semantics";

/// The safe area as the viewport's edges, which the text-input host's padding
/// computes from (`env()` resolves at computed-value time, so the computed
/// padding is the inset in CSS pixels). A browser without `env()` drops the
/// declaration and computes no padding.
const SAFE_AREA_PADDING: [(&str, &str); 4] = [
    ("padding-top", "env(safe-area-inset-top, 0px)"),
    ("padding-right", "env(safe-area-inset-right, 0px)"),
    ("padding-bottom", "env(safe-area-inset-bottom, 0px)"),
    ("padding-left", "env(safe-area-inset-left, 0px)"),
];

/// One canvas window's elements in the page; see the module doc.
pub(crate) struct PageRoot {
    canvas: HtmlCanvasElement,
    slots: HostSlots,
    owner: Owner,
}

/// Who owns the canvas, and so what retiring takes away.
enum Owner {
    /// The host made `root`, the canvas in it and the two hosts.
    Host { root: HtmlElement },
    /// The page's canvas, and what it had before the window wrote to it.
    Page {
        style: Option<String>,
        tab_index: Option<String>,
    },
}

impl PageRoot {
    /// The page's own canvas, `<canvas id="flui-canvas">`, when it supplies
    /// one; `None` when it supplies none, or when the canvas with that id is
    /// one another window's host made (another FLUI instance on the page).
    ///
    /// # Errors
    /// The element with that id is not a canvas.
    pub(crate) fn page_canvas(document: &Document) -> Result<Option<HtmlCanvasElement>, String> {
        let Some(element) = document.get_element_by_id(PAGE_CANVAS_ID) else {
            return Ok(None);
        };
        let made_by_a_host = element
            .parent_element()
            .is_some_and(|parent| parent.tag_name().eq_ignore_ascii_case(ROOT_TAG));
        if made_by_a_host {
            return Ok(None);
        }
        element
            .dyn_into::<HtmlCanvasElement>()
            .map(Some)
            .map_err(|_| format!("element '{PAGE_CANVAS_ID}' is not a canvas"))
    }

    /// A root of the host's own, appended to `<body>` whole: fixed over the
    /// viewport, the canvas filling it. The canvas takes the id a page's own
    /// would have while no element has it, so a page script finds it there.
    ///
    /// # Errors
    /// The document has no body, or an element cannot be made or inserted.
    pub(crate) fn make(document: &Document) -> Result<Self, JsValue> {
        let body = document
            .body()
            .ok_or_else(|| JsValue::from_str("the document has no body"))?;
        let root: HtmlElement = document.create_element(ROOT_TAG)?.unchecked_into();
        let style = root.style();
        for (property, value) in [
            ("display", "block"),
            ("position", "fixed"),
            ("inset", "0"),
            ("overflow", "hidden"),
        ] {
            style.set_property(property, value)?;
        }
        let canvas: HtmlCanvasElement = document.create_element("canvas")?.unchecked_into();
        if document.get_element_by_id(PAGE_CANVAS_ID).is_none() {
            canvas.set_id(PAGE_CANVAS_ID);
        }
        let style = canvas.style();
        for (property, value) in [("display", "block"), ("width", "100%"), ("height", "100%")] {
            style.set_property(property, value)?;
        }
        let slots = HostSlots::try_new(document)?;
        root.append_child(&canvas)?;
        root.append_child(&slots.text_input)?;
        root.append_child(&slots.semantics)?;
        body.append_child(&root)?;
        Ok(Self {
            canvas,
            slots,
            owner: Owner::Host { root },
        })
    }

    /// The page's `canvas`, with the two hosts inserted after it.
    ///
    /// # Errors
    /// The canvas is not in the document, or an element cannot be made or
    /// inserted.
    pub(crate) fn adopt(document: &Document, canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        let parent = canvas
            .parent_node()
            .ok_or_else(|| JsValue::from_str("the page's canvas is not in the document"))?;
        let owner = Owner::Page {
            style: canvas.get_attribute("style"),
            tab_index: canvas.get_attribute("tabindex"),
        };
        let slots = HostSlots::try_new(document)?;
        parent.insert_before(&slots.text_input, canvas.next_sibling().as_ref())?;
        let placed = Self {
            canvas,
            slots,
            owner,
        };
        parent.insert_before(
            &placed.slots.semantics,
            placed.slots.text_input.next_sibling().as_ref(),
        )?;
        Ok(placed)
    }

    /// Whether the canvas is the page's, whose layout the page owns.
    pub(crate) fn is_page_canvas(&self) -> bool {
        matches!(self.owner, Owner::Page { .. })
    }

    pub(crate) fn canvas(&self) -> &HtmlCanvasElement {
        &self.canvas
    }

    pub(crate) fn slots(&self) -> &HostSlots {
        &self.slots
    }
}

impl Drop for PageRoot {
    fn drop(&mut self) {
        match &self.owner {
            Owner::Host { root } => root.remove(),
            Owner::Page { style, tab_index } => {
                self.slots.text_input.remove();
                self.slots.semantics.remove();
                for (name, value) in [("style", style), ("tabindex", tab_index)] {
                    let restored = match value {
                        Some(value) => self.canvas.set_attribute(name, value),
                        None => self.canvas.remove_attribute(name),
                    };
                    if let Err(error) = restored {
                        tracing::debug!(?error, name, "the page canvas kept the window's value");
                    }
                }
            }
        }
    }
}

impl HostSlots {
    /// The two hosts, not yet in the page.
    fn try_new(document: &Document) -> Result<Self, JsValue> {
        let host = |tag: &str| -> Result<HtmlElement, JsValue> {
            let element: HtmlElement = document.create_element(tag)?.unchecked_into();
            element.style().set_property("display", "contents")?;
            Ok(element)
        };
        let text_input = host(TEXT_INPUT_TAG)?;
        let style = text_input.style();
        for (property, value) in SAFE_AREA_PADDING {
            style.set_property(property, value)?;
        }
        Ok(Self {
            text_input,
            semantics: host(SEMANTICS_TAG)?,
        })
    }

    /// The viewport's unsafe edges in CSS pixels, from the text-input host's
    /// computed padding; `None` where the browser computes none (no
    /// `getComputedStyle`, or a value that is no length).
    pub(crate) fn unsafe_edges(&self) -> Option<EdgeInsets> {
        let style = web_sys::window()?
            .get_computed_style(&self.text_input)
            .ok()
            .flatten()?;
        let edge = |property: &str| {
            let value = style.get_property_value(property).ok()?;
            value.trim().strip_suffix("px")?.trim().parse::<f64>().ok()
        };
        Some(EdgeInsets::new(
            edge("padding-top")?,
            edge("padding-right")?,
            edge("padding-bottom")?,
            edge("padding-left")?,
        ))
    }
}
