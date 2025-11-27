# Scrolling Widgets Specification

## Purpose

This specification references the detailed scrolling widget requirements documented in `crates/flui_widgets/guide/02_scrolling_widgets.md`.

## ADDED Requirements

### Requirement: Scrolling Widget Categories

Scrolling widgets SHALL be organized into three categories: Basic Scrolling, List/Grid Views, and Sliver-Based Scrolling, as documented in guide/02_scrolling_widgets.md.

#### Scenario: Basic scrolling widgets provide single-child scrollable views

**GIVEN** a developer needs to make a single widget scrollable
**WHEN** using basic scrolling widgets (SingleChildScrollView, PageView, ListWheelScrollView, NestedScrollView)
**THEN** widget SHALL support bon builder pattern
**AND** widget SHALL support struct literal pattern
**AND** widget SHALL compose appropriate RenderObject(s) (RenderViewport, RenderListWheelViewport)
**AND** widget SHALL support ScrollController for programmatic control
**AND** widget SHALL support ScrollPhysics for scroll behavior customization
**AND** widget SHALL follow patterns documented in guide/02_scrolling_widgets.md

#### Scenario: List and grid views provide efficient lazy-loading scrollable collections

**GIVEN** a developer needs to display a scrollable list or grid of items
**WHEN** using list/grid widgets (ListView, ListView.builder, ListView.separated, GridView, GridView.count, GridView.extent, GridView.builder)
**THEN** widget SHALL support lazy loading via builder pattern
**AND** widget SHALL support separator insertion (ListView.separated)
**AND** widget SHALL support fixed or variable item extents
**AND** widget SHALL use RenderViewport + RenderSliverList or RenderSliverGrid
**AND** widget SHALL follow patterns documented in guide/02_scrolling_widgets.md

#### Scenario: Sliver-based scrolling enables advanced scroll effects and coordination

**GIVEN** a developer needs advanced scroll effects (collapsing headers, mixed layouts)
**WHEN** using CustomScrollView with Sliver widgets (SliverAppBar, SliverList, SliverGrid, SliverToBoxAdapter, SliverFillRemaining, SliverPadding, SliverPersistentHeader, SliverFixedExtentList, SliverPrototypeExtentList, SliverOpacity, SliverIgnorePointer, SliverOffstage)
**THEN** widget SHALL support slivers list composition
**AND** widget SHALL coordinate multiple sliver layouts
**AND** widget SHALL support persistent/floating/pinned headers
**AND** widget SHALL use RenderViewport with various RenderSliver* objects
**AND** widget SHALL follow patterns documented in guide/02_scrolling_widgets.md

---

### Requirement: ScrollController and ScrollPhysics Integration

Scrolling widgets SHALL integrate with ScrollController for programmatic control and ScrollPhysics for scroll behavior.

#### Scenario: ScrollController provides programmatic scroll control

**GIVEN** a scrolling widget with ScrollController
**WHEN** developer needs to programmatically control scroll position
**THEN** widget SHALL accept ScrollController parameter
**AND** controller SHALL expose scroll position, offset, and metrics
**AND** controller SHALL support animateTo() and jumpTo() methods
**AND** controller SHALL work across all scrolling widget types

#### Scenario: ScrollPhysics defines scroll behavior

**GIVEN** a scrolling widget with custom physics
**WHEN** developer needs platform-specific or custom scroll behavior
**THEN** widget SHALL accept ScrollPhysics parameter
**AND** physics SHALL control overscroll, fling, and snapping behavior
**AND** built-in physics SHALL include ClampingScrollPhysics, BouncingScrollPhysics, AlwaysScrollableScrollPhysics, NeverScrollableScrollPhysics
**AND** custom physics SHALL be composable via parent parameter

---

### Requirement: Scrollbar and Notification Integration

Scrolling widgets SHALL integrate with Scrollbar for visual feedback and NotificationListener for scroll events.

#### Scenario: Scrollbar provides visual scroll indicator

**GIVEN** a scrolling widget wrapped in Scrollbar
**WHEN** content exceeds viewport size
**THEN** Scrollbar SHALL render thumb and track overlays
**AND** Scrollbar SHALL respond to drag gestures
**AND** Scrollbar SHALL support thumb visibility, track visibility, thickness, radius, and orientation
**AND** Scrollbar SHALL use RenderMouseRegion + RenderIgnorePointer for thumb

#### Scenario: NotificationListener captures scroll events

**GIVEN** a scrolling widget wrapped in NotificationListener
**WHEN** scroll events occur (start, update, end, overscroll)
**THEN** NotificationListener SHALL receive ScrollNotification events
**AND** notifications SHALL include scroll metrics (pixels, extent, maxScrollExtent, minScrollExtent)
**AND** listener SHALL support event bubbling control via return value
**AND** listener SHALL not modify rendering (uses RenderProxyBox)

---

## Related Documentation

- **Guide:** `crates/flui_widgets/guide/02_scrolling_widgets.md` - Detailed widget reference
- **Architecture:** `crates/flui_widgets/guide/WIDGET_ARCHITECTURE.md` - Widget organization patterns
- **Implementation:** `crates/flui_widgets/guide/IMPLEMENTATION_GUIDE.md` - Code examples

## Widgets Covered

**Total:** 24 scrolling widgets

**Basic Scrolling (5):**
- SingleChildScrollView, PageView, PageView.builder, PageView.custom
- ListWheelScrollView, NestedScrollView

**List/Grid Views (7):**
- ListView, ListView.builder, ListView.separated, ListView.custom
- GridView, GridView.count, GridView.extent, GridView.builder, GridView.custom

**Sliver Widgets (12):**
- SliverAppBar, SliverList, SliverGrid, SliverToBoxAdapter
- SliverFillRemaining, SliverPadding, SliverPersistentHeader
- SliverFixedExtentList, SliverPrototypeExtentList
- SliverOpacity, SliverIgnorePointer, SliverOffstage
- CustomScrollView

**Utilities (2):**
- Scrollbar, NotificationListener
