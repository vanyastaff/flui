# FLUI Master Implementation Roadmap

**Version:** 1.0  
**Last Updated:** January 25, 2026  
**Status:** Planning Phase

---

## Executive Summary

This master roadmap consolidates platform-specific improvements from 5 platform roadmaps (macOS, Windows, Linux, Android, Web) into actionable implementation plans for each FLUI crate.

**Platform Coverage:**
- ✅ macOS (Tahoe 26, Sequoia 15)
- ✅ Windows (11 24H2+, App SDK 1.6-1.8)
- ✅ Linux (Wayland, Vulkan 1.4, Mesa 25.x)
- ✅ Android (15, 16 "Baklava")
- ✅ Web (WebGPU, WebAssembly 3.0, WASI 0.3)

**Total Features Identified:** 100+ platform-specific improvements  
**Implementation Timeline:** Q1 2026 - Q4 2026 (48 weeks)  
**Estimated Budget:** $1.1M - $2.1M (10-15 engineers)

---

## Crate Implementation Plans

### Core Infrastructure Crates

| Crate | Purpose | Priority | Plan Document |
|-------|---------|----------|---------------|
| **flui-platform** | Window management, input, OS integration | ⭐⭐⭐⭐⭐ | [FLUI_PLATFORM_IMPLEMENTATION_PLAN.md](./FLUI_PLATFORM_IMPLEMENTATION_PLAN.md) |
| **flui_engine** | GPU rendering (Metal, DX12, Vulkan, WebGPU) | ⭐⭐⭐⭐⭐ | [FLUI_ENGINE_IMPLEMENTATION_PLAN.md](./FLUI_ENGINE_IMPLEMENTATION_PLAN.md) |
| **flui_painting** | 2D graphics, text, images, effects | ⭐⭐⭐⭐⭐ | [FLUI_PAINTING_IMPLEMENTATION_PLAN.md](./FLUI_PAINTING_IMPLEMENTATION_PLAN.md) |

### Widget & Application Crates

| Crate | Purpose | Priority | Status |
|-------|---------|----------|--------|
| **flui_widgets** | UI widgets (buttons, text fields, etc.) | ⭐⭐⭐⭐⭐ | Q2 2026 |
| **flui-view** | View trait, element tree | ⭐⭐⭐⭐⭐ | Active |
| **flui-reactivity** | State management, signals | ⭐⭐⭐⭐⭐ | Active |
| **flui_animation** | Animation system | ⭐⭐⭐⭐ | Q2 2026 |
| **flui_interaction** | Gestures, hit testing | ⭐⭐⭐⭐ | Active |

### Future Crates (New)

| Crate | Purpose | Priority | Timeline |
|-------|---------|----------|----------|
| **flui-ai** | On-device ML (Core ML, Windows ML, TensorFlow Lite) | ⭐⭐⭐ | Q4 2026 |
| **flui_media** | Video/audio (WebCodecs, platform codecs) | ⭐⭐⭐ | Q3 2026 |
| **flui-network** | WebTransport, WebRTC | ⭐⭐ | Q4 2026 |
| **flui-plugin** | WASI Component Model plugins | ⭐⭐⭐ | Q4 2026 |

---

## Platform Feature Matrix

### macOS Features

| Feature | Crate | Priority | Quarter |
|---------|-------|----------|---------|
| **Liquid Glass Materials** | flui_painting | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **Metal 4 Integration** | flui_engine | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **Window Tiling API** | flui-platform | ⭐⭐⭐⭐ | Q1 2026 |
| **NSHostingMenu** | flui-platform | ⭐⭐⭐ | Q2 2026 |
| **SF Symbols Layered** | flui_painting | ⭐⭐⭐⭐ | Q2 2026 |
| **Accessibility (NSAccessibility)** | flui-platform | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **EDR (Extended Dynamic Range)** | flui_engine | ⭐⭐⭐⭐ | Q2 2026 |

**Total macOS Features:** 15  
**Q1 2026 Focus:** Liquid Glass, Metal 4, Window Tiling, Accessibility

---

### Windows Features

| Feature | Crate | Priority | Quarter |
|---------|-------|----------|---------|
| **DirectX 12 Work Graphs** | flui_engine | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **WinUI 3 Content Islands** | flui-platform | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **Snap Layouts** | flui-platform | ⭐⭐⭐⭐ | Q1 2026 |
| **Auto HDR** | flui_engine | ⭐⭐⭐⭐ | Q2 2026 |
| **Windows ML** | flui-ai | ⭐⭐⭐ | Q4 2026 |
| **File Picker (WinUI 3)** | flui-platform | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **Acrylic/Mica Materials** | flui_painting | ⭐⭐⭐⭐ | Q2 2026 |

**Total Windows Features:** 18  
**Q1 2026 Focus:** DirectX 12, WinUI 3, Snap Layouts, File Picker

---

### Linux Features

| Feature | Crate | Priority | Quarter |
|---------|-------|----------|---------|
| **Wayland Platform** | flui-platform | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **Vulkan 1.4 (Mesa 25.x)** | flui_engine | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **NVIDIA Explicit Sync** | flui-platform | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **Fractional Scaling** | flui-platform | ⭐⭐⭐⭐ | Q1 2026 |
| **XDG File Picker Portal** | flui-platform | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **PipeWire Audio** | flui-platform | ⭐⭐⭐⭐⭐ | Q2 2026 |
| **Screen Capture Portal** | flui-platform | ⭐⭐⭐⭐ | Q2 2026 |
| **cosmic-text Integration** | flui_painting | ⭐⭐⭐⭐ | Q2 2026 |
| **VRR (Variable Refresh Rate)** | flui-platform | ⭐⭐⭐⭐ | Q3 2026 |

**Total Linux Features:** 22  
**Q1 2026 Focus:** Wayland, Vulkan 1.4, NVIDIA Sync, XDG Portals

---

### Android Features

| Feature | Crate | Priority | Quarter |
|---------|-------|----------|---------|
| **16KB Page Size Support** | flui-platform | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **NDK r26 Migration** | Build system | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **Embedded Photo Picker** | flui-platform | ⭐⭐⭐⭐⭐ | Q2 2026 |
| **Desktop Mode (Tablets)** | flui-platform | ⭐⭐⭐⭐ | Q2 2026 |
| **Vulkan 1.3** | flui_engine | ⭐⭐⭐⭐ | Q2 2026 |
| **Progress Notifications** | flui-platform | ⭐⭐⭐⭐ | Q2 2026 |
| **JNI Cache Optimization** | flui-platform | ⭐⭐⭐ | Q2 2026 |
| **Edge-to-Edge Rendering** | flui-platform | ⭐⭐⭐ | Q2 2026 |

**Total Android Features:** 16  
**Q1 2026 Focus:** 16KB Pages (CRITICAL), NDK r26

---

### Web Features

| Feature | Crate | Priority | Quarter |
|---------|-------|----------|---------|
| **WebGPU Rendering** | flui_engine | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **wasm-bindgen Integration** | flui-platform | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **PWA Manifest** | Static assets | ⭐⭐⭐⭐⭐ | Q1 2026 |
| **WASM Threads (SharedArrayBuffer)** | flui-platform | ⭐⭐⭐⭐⭐ | Q2 2026 |
| **Binary Size Optimization** | Build system | ⭐⭐⭐⭐ | Q2 2026 |
| **IndexedDB Storage** | flui-platform | ⭐⭐⭐⭐ | Q2 2026 |
| **Service Worker Caching** | JavaScript | ⭐⭐⭐⭐ | Q3 2026 |
| **WebCodecs Video** | flui_media | ⭐⭐⭐ | Q3 2026 |
| **WebTransport** | flui-network | ⭐⭐⭐ | Q3 2026 |

**Total Web Features:** 17  
**Q1 2026 Focus:** WebGPU, wasm-bindgen, PWA

---

## Quarterly Breakdown

### Q1 2026: Foundation (Weeks 1-12)

**Goal:** Core platform support across all 5 platforms

#### Critical Path
1. **flui-platform** (8 weeks)
   - macOS: Window + Input (DONE Phase 8.6), Liquid Glass (NEW), Accessibility
   - Windows: HWND + WinUI 3 Content Islands, File Picker
   - Linux: Wayland window + XDG Portals
   - Android: 16KB page size (CRITICAL), NDK r26
   - Web: wasm-bindgen + Canvas

2. **flui_engine** (8 weeks)
   - Metal 4 backend (macOS)
   - DirectX 12 Agility SDK (Windows)
   - Vulkan 1.4 (Linux)
   - WebGPU (Web)
   - wgpu integration finalized

3. **flui_painting** (6 weeks)
   - Canvas API
   - Path tessellation (Lyon)
   - Text shaping (cosmic-text preferred)
   - Image loading

**Deliverables:**
- ✅ Window creation on all platforms
- ✅ Basic rendering (rectangles, text)
- ✅ Input events (keyboard, mouse, touch)
- ✅ Platform-specific features (Liquid Glass, WinUI 3, Wayland)

**Team:** 10 engineers (2 per platform)

---

### Q2 2026: Features & Polish (Weeks 13-24)

**Goal:** Platform-specific features, optimization

#### Critical Path
1. **flui-platform** (6 weeks)
   - Android: Embedded Photo Picker, Desktop Mode
   - Linux: PipeWire audio, Screen Capture Portal
   - Web: WASM threads, IndexedDB
   - All: Clipboard, Drag & Drop abstraction

2. **flui_engine** (6 weeks)
   - Shader system (WGSL compilation, caching)
   - GPU effects (blur, shadow)
   - HDR support (macOS EDR, Windows Auto HDR)
   - Texture atlas

3. **flui_painting** (4 weeks)
   - Gradients (linear, radial, conic)
   - Platform materials (Liquid Glass, Acrylic/Mica)
   - Image cache
   - Display list optimization

4. **flui_widgets** (8 weeks)
   - Basic widgets (Button, Text, TextField, Image)
   - Layout widgets (Row, Column, Stack, Container)
   - Platform-specific widgets (TabView, SegmentedControl)

**Deliverables:**
- ✅ Full widget library
- ✅ GPU-accelerated effects
- ✅ Platform materials (Liquid Glass, Acrylic)
- ✅ Advanced input (clipboard, drag-drop)

**Team:** 12 engineers (widgets team added)

---

### Q3 2026: Optimization & Advanced Features (Weeks 25-36)

**Goal:** Performance optimization, advanced integrations

#### Critical Path
1. **Performance Optimization** (6 weeks)
   - GPU profiling (timestamp queries)
   - Render batching/instancing
   - Memory optimization
   - Platform-specific optimizations

2. **Advanced Features** (6 weeks)
   - Linux: VRR, cosmic-text, real-time threads
   - Web: Service Workers, WebCodecs, WebTransport
   - Android: JNI cache, ANGLE detection
   - macOS: SF Symbols, NSHostingMenu

3. **flui_animation** (4 weeks)
   - Tween system
   - Physics-based animations
   - Implicit animations
   - Stagger/sequence

4. **flui_media** (4 weeks)
   - Video playback (WebCodecs, platform codecs)
   - Audio playback (PipeWire, Web Audio)
   - Camera input

**Deliverables:**
- ✅ 60 FPS @ 4K on all platforms
- ✅ Advanced platform features
- ✅ Animation system
- ✅ Media playback

**Team:** 10 engineers

---

### Q4 2026: Advanced Integrations & Polish (Weeks 37-48)

**Goal:** AI/ML, plugins, final polish

#### Critical Path
1. **flui-ai** (6 weeks)
   - macOS: Core ML integration
   - Windows: Windows ML (DirectML)
   - Android: TensorFlow Lite
   - Web: WebGPU compute shaders
   - Linux: ONNX Runtime

2. **flui-plugin** (6 weeks)
   - WASI 0.3 Component Model
   - Plugin loading/unloading
   - Sandboxing
   - Example plugins

3. **flui-network** (4 weeks)
   - WebTransport (Chrome/Edge)
   - WebRTC (all browsers)
   - HTTP client

4. **Final Polish** (6 weeks)
   - Bug fixes across all platforms
   - Performance tuning
   - Documentation
   - Example apps

**Deliverables:**
- ✅ On-device ML support
- ✅ Plugin system
- ✅ Networking capabilities
- ✅ Production-ready quality

**Team:** 8 engineers

---

## Budget & Resources

### Engineering Team (Peak: Q2 2026)

| Role | Count | Duration | Cost/Month | Total |
|------|-------|----------|------------|-------|
| **Platform Engineers** | 6 | 12 months | $15k | $1,080k |
| **Graphics Engineers** | 2 | 9 months | $18k | $324k |
| **Widget Engineers** | 3 | 8 months | $14k | $336k |
| **QA Engineers** | 2 | 12 months | $10k | $240k |
| **DevOps Engineer** | 1 | 6 months | $12k | $72k |

**Total Engineering:** $2,052k

### Hardware & Infrastructure

| Item | Quantity | Cost |
|------|----------|------|
| **macOS Devices** | 4 (M4, M1, Intel, iPad) | $8k |
| **Windows Devices** | 3 (AMD, NVIDIA, Surface) | $5k |
| **Linux Workstations** | 3 (AMD, Intel, NVIDIA) | $4k |
| **Android Devices** | 5 (Pixel, Galaxy, Tablet) | $4k |
| **CI/CD Infrastructure** | Cloud (GitHub Actions) | $2k/year |
| **BrowserStack** | Cross-browser testing | $1.2k/year |

**Total Hardware:** $24k  
**Total Infrastructure:** $3.2k/year

### Grand Total

- **Engineering:** $2,052k
- **Hardware:** $24k
- **Infrastructure:** $3.2k
- **Contingency (10%):** $208k

**Total Budget:** **$2,287k** (~$2.3M)

---

## Risk Assessment

### Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| **Android 16KB page size issues** | High | Critical | Early testing on API 35+ devices, August 2025 deadline |
| **Cross-origin isolation (Web)** | Medium | High | Documentation, dev server template, examples |
| **Wayland compositor incompatibilities** | Medium | High | Test on multiple compositors (Mutter, KWin, Sway, COSMIC) |
| **wgpu version fragmentation** | Low | Medium | Stay on wgpu 25.x (26.0+ has issues) |
| **NVIDIA driver regressions** | Low | High | Support both proprietary (590+) and NVK |
| **macOS Liquid Glass API changes** | Low | Medium | Monitor beta releases, Apple dev forums |

### Timeline Risks

| Milestone | Target Date | Risk | Mitigation |
|-----------|-------------|------|------------|
| **Q1 2026 Completion** | March 31, 2026 | Medium | Critical path focus, weekly reviews |
| **Android API 35 Deadline** | August 2025 | High | ALREADY PASSED! Must test 16KB immediately |
| **WASI 0.3 Release** | February 2026 | Low | Expected on schedule |
| **WebGPU Stability** | Ongoing | Low | Universal support achieved Nov 2025 |

---

## Success Metrics

### Performance Targets

| Metric | Target | Platform | Status |
|--------|--------|----------|--------|
| **Frame Rate** | 60 FPS @ 4K | All | In Progress |
| **Input Latency** | < 10ms | All | To Test |
| **Startup Time** | < 500ms | Desktop | To Test |
| **Binary Size (WASM)** | < 500KB | Web | To Optimize |
| **Memory Usage** | < 100MB | All | To Profile |
| **GPU Memory** | < 100MB | All | To Profile |

### Platform Certification

| Platform | Certification | Status |
|----------|--------------|--------|
| **macOS** | Notarization | Q3 2026 |
| **Windows** | App Cert Kit | Q3 2026 |
| **Linux** | Flathub | Q4 2026 |
| **Android** | Play Store | Q3 2026 |
| **Web** | PWA Lighthouse 90+ | Q3 2026 |

### Accessibility

| Platform | Standard | Status |
|----------|----------|--------|
| **macOS** | VoiceOver 100% | Q2 2026 |
| **Windows** | Narrator 100% | Q2 2026 |
| **Linux** | Orca 100% | Q3 2026 |
| **Android** | TalkBack 100% | Q3 2026 |
| **Web** | WCAG 2.1 AA | Q2 2026 |

---

## Dependencies & Blockers

### External Dependencies

| Dependency | Required For | Status | Risk |
|------------|--------------|--------|------|
| **wgpu 25.x** | All rendering | ✅ Stable | Low (stay on 25.x) |
| **WASI 0.3** | Web plugins | ⏳ Feb 2026 | Low (on track) |
| **Mesa 25.x** | Linux Vulkan 1.4 | ✅ Released | Low |
| **NDK r26** | Android 16KB pages | ✅ Released | Medium (migration needed) |
| **Windows App SDK 1.8** | WinUI 3 | ✅ Released | Low |

### Internal Blockers

| Blocker | Blocks | Timeline | Owner |
|---------|--------|----------|-------|
| **flui-tree completion** | All widget rendering | Active | Core team |
| **flui-reactivity completion** | State management | Active | Core team |
| **flui-scheduler completion** | Animations | Q1 2026 | Core team |

---

## Communication & Reporting

### Weekly Standups
- **Time:** Mondays 10am PST
- **Duration:** 30 minutes
- **Attendees:** All engineers
- **Format:** Blockers, progress, plans

### Bi-Weekly Demos
- **Time:** Fridays 2pm PST
- **Duration:** 1 hour
- **Attendees:** Full team + stakeholders
- **Format:** Live demos on each platform

### Monthly Reviews
- **Time:** Last Friday of month
- **Duration:** 2 hours
- **Attendees:** Leadership + team
- **Format:** Progress against milestones, budget review

---

## Next Steps (Immediate Actions)

### Week 1 (Jan 27 - Jan 31, 2026)
1. ✅ **Review and approve master roadmap** - Leadership
2. ✅ **Assign platform owners** - Engineering Manager
3. ✅ **Set up development environments** - All engineers
4. ✅ **Create cross-platform abstractions** - Core team (Window trait, Event types)
5. ⚠️ **URGENT: Android 16KB page testing** - Android team (deadline already passed!)

### Week 2 (Feb 3 - Feb 7, 2026)
1. **Begin flui-platform implementation** - Platform teams
2. **Begin flui_engine wgpu integration** - Graphics team
3. **Set up CI/CD pipelines** - DevOps
4. **Order hardware** - Manager
5. **Create example apps** - All teams (one per platform)

### Week 3 (Feb 10 - Feb 14, 2026)
1. **First platform demos** - Each team shows window + input
2. **Performance baseline measurements** - QA team
3. **Documentation framework** - Tech writer
4. **Weekly standup rhythm established** - All teams

---

## Appendices

### A. Platform Roadmap References
- [macOS Improvements Roadmap](../roadmap/MACOS_IMPROVEMENTS_ROADMAP.md)
- [Windows Improvements Roadmap](../roadmap/WINDOWS_IMPROVEMENTS_ROADMAP.md)
- [Linux Improvements Roadmap](../roadmap/LINUX_IMPROVEMENTS_ROADMAP.md)
- [Android Improvements Roadmap](../roadmap/ANDROID_IMPROVEMENTS_ROADMAP.md)
- [Web Improvements Roadmap](../roadmap/WEB_IMPROVEMENTS_ROADMAP.md)

### B. Crate Implementation Plans
- [flui-platform Implementation Plan](./FLUI_PLATFORM_IMPLEMENTATION_PLAN.md)
- [flui_engine Implementation Plan](./FLUI_ENGINE_IMPLEMENTATION_PLAN.md)
- [flui_painting Implementation Plan](./FLUI_PAINTING_IMPLEMENTATION_PLAN.md)

### C. Technical Documentation
- [Project Philosophy](../PROJECT_PHILOSOPHY.md)
- [Architecture Overview](../ARCHITECTURE_OVERVIEW.md)
- [API Quick Reference](../API_QUICK_REFERENCE.md)

---

**Document Status:** ✅ Complete  
**Last Review:** January 25, 2026  
**Next Review:** February 1, 2026
