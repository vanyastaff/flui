# Input and Buttons Specification

## Purpose

This specification references the detailed input and button widget requirements documented in `crates/flui_widgets/guide/06_input_and_buttons.md`.

## ADDED Requirements

### Requirement: Input Widget Categories

Input widgets SHALL be organized into text input, selection controls, and dropdown categories, as documented in guide/06_input_and_buttons.md.

#### Scenario: TextField provides text input with decoration

**GIVEN** a developer needs text input field
**WHEN** using TextField widget
**THEN** widget SHALL support bon builder pattern
**AND** widget SHALL support struct literal pattern
**AND** widget SHALL use RenderEditable for rendering
**AND** widget SHALL support controller (TextEditingController), focusNode, decoration (InputDecoration)
**AND** widget SHALL support keyboardType, textInputAction, textCapitalization, style, textAlign
**AND** widget SHALL support readOnly, obscureText, autocorrect, maxLines, minLines, expands, maxLength
**AND** widget SHALL support onChanged, onSubmitted, onEditingComplete callbacks
**AND** widget SHALL support enabled, cursorColor, scrollPadding, enableInteractiveSelection
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: TextFormField integrates TextField with Form validation

**GIVEN** a developer needs text input with form validation
**WHEN** using TextFormField widget
**THEN** widget SHALL extend FormField<String>
**AND** widget SHALL use RenderEditable for rendering
**AND** widget SHALL support all TextField parameters
**AND** widget SHALL support initialValue, validator, onSaved, autovalidateMode, restorationId
**AND** widget SHALL integrate with Form.of(context) for validation and save
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

---

### Requirement: Selection Control Widgets

Selection control widgets SHALL provide checkboxes, radio buttons, switches, and sliders, as documented in guide/06_input_and_buttons.md.

#### Scenario: Checkbox provides boolean selection with indeterminate state

**GIVEN** a developer needs checkbox control
**WHEN** using Checkbox widget
**THEN** widget SHALL use RenderCustomPaint + RenderInkFeatures for rendering
**AND** widget SHALL support value (bool?), onChanged callback
**AND** widget SHALL support tristate parameter for null value (indeterminate)
**AND** widget SHALL support activeColor, checkColor, fillColor, focusColor, hoverColor
**AND** widget SHALL support splashRadius, materialTapTargetSize, visualDensity, shape, side
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: CheckboxListTile combines ListTile with Checkbox

**GIVEN** a developer needs checkbox with label
**WHEN** using CheckboxListTile widget
**THEN** widget SHALL combine ListTile and Checkbox rendering
**AND** widget SHALL support value, onChanged, tristate from Checkbox
**AND** widget SHALL support title, subtitle, secondary, isThreeLine, dense, selected from ListTile
**AND** widget SHALL support controlAffinity (leading, trailing, platform)
**AND** widget SHALL support activeColor, checkColor, tileColor, selectedTileColor, contentPadding
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: Radio provides mutually exclusive selection

**GIVEN** a developer needs radio button within group
**WHEN** using Radio<T> widget
**THEN** widget SHALL use RenderCustomPaint + RenderInkFeatures for rendering
**AND** widget SHALL support value (T), groupValue (T?), onChanged callback
**AND** widget SHALL support toggleable parameter for deselection
**AND** widget SHALL support activeColor, fillColor, focusColor, hoverColor, overlayColor
**AND** widget SHALL support splashRadius, materialTapTargetSize, visualDensity
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: RadioListTile combines ListTile with Radio

**GIVEN** a developer needs radio button with label
**WHEN** using RadioListTile<T> widget
**THEN** widget SHALL combine ListTile and Radio rendering
**AND** widget SHALL support value, groupValue, onChanged, toggleable from Radio
**AND** widget SHALL support title, subtitle, secondary, controlAffinity from CheckboxListTile
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: Switch provides toggle control

**GIVEN** a developer needs toggle switch
**WHEN** using Switch widget
**THEN** widget SHALL use RenderCustomPaint + RenderInkFeatures for rendering
**AND** widget SHALL support value (bool), onChanged callback
**AND** widget SHALL support activeColor, activeTrackColor, inactiveThumbColor, inactiveTrackColor
**AND** widget SHALL support thumbColor, trackColor, trackOutlineColor, thumbIcon (MaterialStateProperty)
**AND** widget SHALL support activeThumbImage, inactiveThumbImage
**AND** widget SHALL support materialTapTargetSize, dragStartBehavior, splashRadius
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: SwitchListTile combines ListTile with Switch

**GIVEN** a developer needs switch with label
**WHEN** using SwitchListTile widget
**THEN** widget SHALL combine ListTile and Switch rendering
**AND** widget SHALL support value, onChanged from Switch
**AND** widget SHALL support activeColor, activeTrackColor, inactiveThumbColor, inactiveTrackColor
**AND** widget SHALL support title, subtitle, secondary, controlAffinity from CheckboxListTile
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: Slider provides continuous value selection

**GIVEN** a developer needs continuous value slider
**WHEN** using Slider widget
**THEN** widget SHALL use RenderCustomPaint for track and thumb
**AND** widget SHALL support value, onChanged, onChangeStart, onChangeEnd callbacks
**AND** widget SHALL support min, max (double), divisions (int?) for discrete steps
**AND** widget SHALL support label (String) shown above thumb
**AND** widget SHALL support activeColor, inactiveColor, thumbColor, overlayColor
**AND** widget SHALL support Slider.adaptive() for platform-specific rendering
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: RangeSlider provides range selection with two thumbs

**GIVEN** a developer needs range selection
**WHEN** using RangeSlider widget
**THEN** widget SHALL use RenderCustomPaint for track and two thumbs
**AND** widget SHALL support values (RangeValues), onChanged, onChangeStart, onChangeEnd
**AND** widget SHALL support min, max, divisions
**AND** widget SHALL support labels (RangeLabels) for start and end labels
**AND** widget SHALL support activeColor, inactiveColor
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

---

### Requirement: Dropdown Widgets

Dropdown widgets SHALL provide dropdown menus with item selection, as documented in guide/06_input_and_buttons.md.

#### Scenario: DropdownButton provides dropdown menu

**GIVEN** a developer needs dropdown selection
**WHEN** using DropdownButton<T> widget
**THEN** widget SHALL use RenderFlex + overlay for menu
**AND** widget SHALL support items (List<DropdownMenuItem<T>>), value (T?), onChanged callback
**AND** widget SHALL support onTap, selectedItemBuilder, hint, disabledHint
**AND** widget SHALL support elevation, style, icon, iconDisabledColor, iconEnabledColor, iconSize
**AND** widget SHALL support isDense, isExpanded, itemHeight, focusColor, dropdownColor
**AND** widget SHALL support menuMaxHeight, enableFeedback, alignment, borderRadius, padding
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: DropdownMenuItem provides menu item

**GIVEN** a DropdownButton needs menu items
**WHEN** using DropdownMenuItem<T> widget
**THEN** widget SHALL use RenderPadding + RenderInkFeatures
**AND** widget SHALL support value (T), onTap, enabled, alignment, child
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: DropdownButtonFormField integrates with Form

**GIVEN** a developer needs dropdown with form validation
**WHEN** using DropdownButtonFormField<T> widget
**THEN** widget SHALL extend FormField<T>
**AND** widget SHALL support all DropdownButton parameters
**AND** widget SHALL support decoration, validator, onSaved, autovalidateMode
**AND** widget SHALL integrate with Form validation
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

---

### Requirement: Button Widget Categories

Button widgets SHALL provide Material Design and Cupertino button styles, as documented in guide/06_input_and_buttons.md.

#### Scenario: Material Design buttons provide standard button styles

**GIVEN** a developer needs Material Design button
**WHEN** using TextButton, ElevatedButton, OutlinedButton, IconButton, or FloatingActionButton
**THEN** widget SHALL use RenderPhysicalModel + RenderInkFeatures + RenderFlex (except IconButton)
**AND** widget SHALL support onPressed, onLongPress callbacks (null = disabled)
**AND** widget SHALL support onHover, onFocusChange callbacks
**AND** widget SHALL support style (ButtonStyle), focusNode, autofocus, clipBehavior
**AND** TextButton SHALL provide flat button without elevation
**AND** ElevatedButton SHALL provide raised button with elevation and shadow
**AND** OutlinedButton SHALL provide button with border outline
**AND** IconButton SHALL provide icon-only button
**AND** FloatingActionButton SHALL use RenderPhysicalShape for circular elevation
**AND** all button types SHALL support .icon() variant (except IconButton)
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: FloatingActionButton provides floating action button

**GIVEN** a developer needs floating action button
**WHEN** using FloatingActionButton widget
**THEN** widget SHALL use RenderPhysicalShape (circular) + RenderInkFeatures
**AND** widget SHALL support onPressed, tooltip, foregroundColor, backgroundColor
**AND** widget SHALL support elevation, focusElevation, hoverElevation, highlightElevation, disabledElevation
**AND** widget SHALL support shape, clipBehavior, mini, mouseCursor, heroTag
**AND** widget SHALL support FloatingActionButton.extended() for text + icon
**AND** widget SHALL support FloatingActionButton.small() and FloatingActionButton.large()
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

#### Scenario: CupertinoButton provides iOS-style button

**GIVEN** a developer needs iOS-style button
**WHEN** using CupertinoButton widget
**THEN** widget SHALL use RenderPointerListener + RenderOpacity + RenderPadding
**AND** widget SHALL support onPressed, child, padding, color, disabledColor
**AND** widget SHALL support minSize, pressedOpacity, borderRadius, alignment
**AND** widget SHALL apply opacity fade on press instead of ripple
**AND** widget SHALL support CupertinoButton.filled() for background
**AND** widget SHALL follow patterns documented in guide/06_input_and_buttons.md

---

## Related Documentation

- **Guide:** `crates/flui_widgets/guide/06_input_and_buttons.md` - Detailed widget reference
- **Architecture:** `crates/flui_widgets/guide/WIDGET_ARCHITECTURE.md` - Widget organization patterns
- **Implementation:** `crates/flui_widgets/guide/IMPLEMENTATION_GUIDE.md` - Code examples

## Widgets Covered

**Total:** 22 input and button widgets

**Text Input (2):**
- TextField, TextFormField

**Selection Controls (8):**
- Checkbox, CheckboxListTile
- Radio<T>, RadioListTile<T>
- Switch, SwitchListTile
- Slider, RangeSlider

**Dropdown (3):**
- DropdownButton<T>, DropdownMenuItem<T>, DropdownButtonFormField<T>

**Material Buttons (5):**
- TextButton, ElevatedButton, OutlinedButton
- IconButton, FloatingActionButton

**Cupertino Buttons (1):**
- CupertinoButton

**Supporting Types (not widgets):**
- TextEditingController
- InputDecoration
- ButtonStyle
- RangeValues, RangeLabels
