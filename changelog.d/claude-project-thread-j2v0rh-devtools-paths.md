### Removed

- `flui_devtools` modules are private: `flui_devtools::capture::…`, `flui_devtools::timeline::…` and the six other module paths no longer exist, and neither does `flui_devtools::prelude`.
- Every public item is at the crate root under one path, for example `flui_devtools::Timeline`, `flui_devtools::Profiler` and `flui_devtools::InspectorCounters`.
- The gesture overlay's fade duration, `flui_devtools::gesture_overlay::FADE`, is `flui_devtools::CONTEST_FADE`.
