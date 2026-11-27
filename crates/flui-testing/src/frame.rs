//! A frame's internals, read in process through types (ADR-0400 §4): [`Frame`], what
//! [`Tester::frame`] lends a test of a [`Headless`] connection, and the owned values its reads
//! copy out, [`RenderGeometry`], [`DrawList`] and [`LayerOutline`].
//!
//! A typed read selects render objects by `TypeId` (`downcast_ref::<R>`), in the whole frame or
//! at and under the element a [`Find`] names, requires exactly one where it reads one, and copies
//! an owned value out inside the pipeline's closure-scoped access: no borrow of a render object
//! leaves the frame. The counterparts in Flutter 3.50.1's `flutter_test` are
//! `tester.renderObject<T>(finder)` (`controller.dart:987`) for a field, `getRect` for geometry
//! and the `paints` matcher (`mock_canvas.dart:54`) for drawing, which FLUI reads as an owned
//! list instead of a pattern language.
//!
//! A `Frame` exists only on `Tester<Headless<_>>`, so a scenario over a generic
//! [`Connection`](flui_protocol::client::Connection) that reads one does not compile: a device
//! has no frame to lend.
//!
//! ```
//! use flui_objects::RenderParagraph;
//! use flui_testing::scenario::{Tester, find};
//! use flui_widgets::{Center, Text};
//!
//! let mut t = Tester::mount(Center::new().child(Text::new("Hello")));
//! let mut frame = t.frame();
//! let shown = frame
//!     .read::<RenderParagraph, String>(&find::text("Hello"), |paragraph| {
//!         paragraph.painter().text().map_or_default(flui_painting::typography::InlineSpan::to_plain_text)
//!     })
//!     .expect("one paragraph shows the text");
//! assert_eq!(shown, "Hello");
//! ```

mod draw_list;
mod geometry;
mod layers;

use std::any::{Any, type_name};
use std::panic::Location;

use flui_foundation::RenderId;
use flui_protocol::locator::Match;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};

pub use self::draw_list::DrawList;
pub(crate) use self::draw_list::scene_commands;
pub use self::geometry::RenderGeometry;
pub use self::layers::LayerOutline;
use crate::FrameReport;
use crate::scenario::{Find, Headless, Observed, Target, TestError, Tester};

/// One presented frame of a [`Headless`] connection, lent by [`Tester::frame`]: nothing acts or
/// pumps while it lives, so every read through it reads the same frame.
///
/// The reads of the whole frame ([`only`](Self::only), [`render_all`](Self::render_all),
/// [`layers`](Self::layers), [`draw_list`](Self::draw_list), [`report`](Self::report)) exist on
/// both depths; the reads at an element a [`Find`] names ([`geometry`](Self::geometry),
/// [`read`](Self::read), [`draw_commands`](Self::draw_commands)) need the window's agent to
/// resolve the find, and so exist on [`Observed`] alone.
pub struct Frame<'t, S>
where
    Headless<S>: Target,
{
    tester: &'t mut Tester<Headless<S>>,
}

impl<S> std::fmt::Debug for Frame<'_, S>
where
    Headless<S>: Target,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("window", &self.tester.window())
            .finish_non_exhaustive()
    }
}

impl<S> Tester<Headless<S>>
where
    Headless<S>: Target,
{
    /// The frame the window presented last, for reads in process (ADR-0400 §4). Lending it is
    /// one owner turn with no frame: the commands queued since the last turn are applied and
    /// nothing is presented.
    pub fn frame(&mut self) -> Frame<'_, S> {
        if let Ok(host) = self.connection_mut().host_mut() {
            host.owner_turn();
        }
        Frame { tester: self }
    }
}

impl<S> Frame<'_, S>
where
    Headless<S>: Target,
{
    /// The geometry of the one render object of type `R` in the frame.
    ///
    /// # Errors
    ///
    /// [`TestError::RenderObjectCount`] unless the frame holds exactly one.
    #[track_caller]
    pub fn only<R: Any>(&mut self) -> Result<RenderGeometry, TestError> {
        let (step, _running) = self.tester.begin("frame.only", Location::caller());
        let found = self.with_pipeline(|owner| geometry::of_type::<R>(owner, None));
        match <[RenderGeometry; 1]>::try_from(found) {
            Ok([one]) => Ok(one),
            Err(found) => Err(TestError::RenderObjectCount {
                render_type: type_name::<R>(),
                find: None,
                found: found.len(),
                diagnosis: Box::new(self.tester.diagnosis(&step, Vec::new(), Vec::new())),
            }),
        }
    }

    /// The geometry of every render object of type `R` in the frame, in tree order.
    #[track_caller]
    #[must_use]
    pub fn render_all<R: Any>(&mut self) -> Vec<RenderGeometry> {
        let (_step, _running) = self.tester.begin("frame.render_all", Location::caller());
        self.with_pipeline(|owner| geometry::of_type::<R>(owner, None))
    }

    /// The layers of the scene the window presented last, depth first.
    #[must_use]
    pub fn layers(&self) -> LayerOutline {
        LayerOutline::of(
            self.tester
                .connection()
                .host()
                .and_then(|host| host.sink().layer_tree()),
        )
    }

    /// Every draw command of the scene the window presented last, in paint order, each placed in
    /// the window.
    #[must_use]
    pub fn draw_list(&self) -> DrawList {
        DrawList::of_scene(
            self.tester
                .connection()
                .host()
                .and_then(|host| host.sink().version()),
            |_| true,
        )
    }

    /// What the frames of the tester's last move of its clock did ([`pump`](Tester::pump),
    /// [`pump_for`](Tester::pump_for) or [`freeze_at`](Tester::freeze_at)), phase by phase: the
    /// counts a perf scenario holds to its baseline (ADR-0396). Empty before the clock first
    /// moves, and after a move on a window that has closed. A step that runs frames without
    /// moving the clock by a duration it names (an action, an `expect`, the end's settle) leaves
    /// it as it was: what such steps cost is [`Tester::measure`]'s.
    #[must_use]
    pub fn report(&self) -> &FrameReport {
        self.tester.connection().advanced()
    }

    /// `read` over the window's pipeline; nothing when the test has ended.
    fn with_pipeline<T: Default>(&self, read: impl FnOnce(&PipelineOwner) -> T) -> T {
        self.pipeline()
            .map_or_else(T::default, |pipeline| pipeline.with(read))
    }

    fn pipeline(&self) -> Option<PipelineCell> {
        self.tester
            .connection()
            .host()?
            .try_ui_runtime()?
            .widgets()
            .pipeline_owner()
    }
}

impl Frame<'_, Observed> {
    /// The geometry of the one render object of type `R` at or under the element `at` names.
    ///
    /// # Errors
    ///
    /// As [`Tester::try_read`] resolves `at`; [`TestError::NoRenderObject`] when no render
    /// object forms the element; [`TestError::RenderObjectCount`] unless exactly one of type `R`
    /// is at or under it.
    #[track_caller]
    pub fn geometry<R: Any>(&mut self, at: &Find) -> Result<RenderGeometry, TestError> {
        self.one_at::<R, RenderGeometry>("frame.geometry", Location::caller(), at, |owner, id| {
            RenderGeometry::of(owner, id)
        })
    }

    /// `read` of the one render object of type `R` at or under the element `at` names: the value
    /// it copies out, never a borrow.
    ///
    /// # Errors
    ///
    /// As [`Self::geometry`].
    #[track_caller]
    pub fn read<R: Any, T>(&mut self, at: &Find, read: fn(&R) -> T) -> Result<T, TestError> {
        self.one_at::<R, T>("frame.read", Location::caller(), at, |owner, id| {
            let render = owner
                .render_tree()
                .get(id)
                .and_then(|node| node.downcast_render_object::<R>())
                .expect("BUG: the selection found a render object of this type at this id");
            read(render)
        })
    }

    /// The draw commands the element `at` names painted in the scene the window presented last,
    /// in paint order: the render object that forms the element and every render object below it,
    /// as `flutter_test`'s `paints` matcher reads one render object's painting. A painter that
    /// draws outside its own bounds is read whole, and a sibling's drawing is left out, even
    /// where it overlaps.
    ///
    /// Each command's `transform` places it in the window, its canvas transform composed with
    /// every offset and transform of the layers above it; a command under a follower its leader
    /// does not place is left out, as it is not painted.
    ///
    /// # Errors
    ///
    /// As [`Tester::try_read`] resolves `at`; [`TestError::NoRenderObject`] when no render
    /// object forms the element.
    #[track_caller]
    pub fn draw_commands(&mut self, at: &Find) -> Result<DrawList, TestError> {
        let (step, _running) = self.tester.begin("frame.draw_commands", Location::caller());
        let found = self.tester.resolve(&step, at)?;
        let render = self.render_of(&step, at, &found)?;
        let painters = self.with_pipeline(|owner| geometry::subtree(owner, render));
        Ok(DrawList::of_scene(
            self.tester
                .connection()
                .host()
                .and_then(|host| host.sink().version()),
            |painter| painters.contains(&painter),
        ))
    }

    /// The one render object of type `R` at or under the element `at` names, as `copy` reads it.
    fn one_at<R: Any, T>(
        &mut self,
        verb: &'static str,
        caller: &'static Location<'static>,
        at: &Find,
        copy: impl Fn(&PipelineOwner, RenderId) -> T,
    ) -> Result<T, TestError> {
        let (step, _running) = self.tester.begin(verb, caller);
        let found = self.tester.resolve(&step, at)?;
        let render = self.render_of(&step, at, &found)?;
        let copied = self.pipeline().map_or(Err(0), |pipeline| {
            pipeline.with(|owner| {
                let mut found = geometry::ids_of_type::<R>(owner, Some(render));
                match found.len() {
                    1 => Ok(copy(owner, found.remove(0))),
                    count => Err(count),
                }
            })
        });
        copied.map_err(|count| TestError::RenderObjectCount {
            render_type: type_name::<R>(),
            find: Some(Box::new(at.locator().clone())),
            found: count,
            diagnosis: Box::new(self.tester.diagnosis(&step, Vec::new(), Vec::new())),
        })
    }

    /// The render object that forms the element `found` is, which the read starts at.
    fn render_of(
        &mut self,
        step: &crate::scenario::Step,
        at: &Find,
        found: &Match,
    ) -> Result<RenderId, TestError> {
        // A render-formed node's id is its render object's packed id; an authored region's
        // carries a domain bit no render id has.
        let render = RenderId::from_u64(found.element.element.get()).filter(|render| {
            self.with_pipeline(|owner| owner.render_tree().get(*render).is_some())
        });
        render.ok_or_else(|| TestError::NoRenderObject {
            find: Box::new(at.locator().clone()),
            diagnosis: Box::new(self.tester.diagnosis(step, Vec::new(), Vec::new())),
        })
    }
}
