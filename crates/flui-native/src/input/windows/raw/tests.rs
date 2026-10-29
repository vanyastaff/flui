//! The Win32 message decoding (`input::windows`'s `raw`): the message
//! clock's arithmetic, the pointer packets copied into readings, and the
//! mouse buttons' codes.

// Native queue clocks cannot be advanced through a consumer API. These rows
// pin the wrapping/rebasing arithmetic used by the actual Win32 producer;
// queued dispatch itself is covered through a live public window contract.
#[cfg(test)]
mod message_clock_contract {
    use super::super::MessageClock;

    #[test]
    fn message_clock_preserves_wrapping_samples_and_window_epochs() {
        let wrap = u64::from(u32::MAX) + 1;
        for (name, anchor_tick, anchor_ns, now, samples, expected) in [
            (
                "ordinary queued gap",
                100,
                1_000_000,
                200,
                [120, 160],
                [21_000_000, 61_000_000],
            ),
            (
                "32-bit uptime wrap",
                wrap - 10,
                1_000_000,
                wrap + 30,
                [u32::MAX - 4, 7],
                [6_000_000, 18_000_000],
            ),
            (
                "older sent timestamp",
                100,
                1_000_000,
                200,
                [150, 120],
                [51_000_000, 51_000_000],
            ),
            (
                "pre-epoch queued sample",
                100,
                20_000_000,
                200,
                [90, 110],
                [10_000_000, 30_000_000],
            ),
            (
                "nanosecond exhaustion",
                100,
                u64::MAX - 1,
                200,
                [101, 102],
                [u64::MAX, u64::MAX],
            ),
            (
                "native reading ahead of coarse uptime",
                100,
                1_000_000,
                200,
                [200, 201],
                [101_000_000, 102_000_000],
            ),
        ] {
            let clock = MessageClock::anchored(anchor_tick, None, anchor_ns);
            assert_eq!(
                samples.map(|tick| clock.stamp_at(tick, now)),
                expected,
                "{name}"
            );
        }
        let first = MessageClock::anchored(100, None, 1_000_000);
        let second = MessageClock::anchored(120, None, 21_000_000);
        assert_eq!(
            first.stamp_at(150, 200),
            second.stamp_at(150, 200),
            "independent windows share the process epoch"
        );
    }

    /// A message stamped from its millisecond tick after a pointer reading
    /// stamped from its performance count, in the same system tick, is not
    /// placed before that reading.
    #[test]
    fn a_coarse_stamp_never_precedes_a_delivered_precise_one() {
        use flui_platform::EventTime;
        use flui_platform::pointer::{
            CancelReason, PointerCancel, PointerEvent, PointerId, PointerInfo, PointerKind,
        };
        let clock = MessageClock::anchored(100, None, 1_000_000);
        let precise = PointerEvent::Cancel(PointerCancel::new(
            PointerInfo::new(
                PointerId::new(std::num::NonZeroU64::MIN),
                PointerKind::Touch,
            ),
            EventTime::from_nanos(25_400_000),
            CancelReason::Platform,
        ));
        clock.observe(&[flui_platform::PlatformInput::Pointer(precise)]);
        assert_eq!(
            [clock.stamp_at(110, 200), clock.stamp_at(130, 200)],
            [25_400_000, 31_000_000],
            "floored to the reading, then the tick again once it is later"
        );
    }
}
#[cfg(test)]
mod native_pointer_contracts {
    use super::super::*;
    use flui_platform::pointer::{PointerEvent, PointerRole, PointerSample, Pressure};
    use windows::Win32::{
        Foundation::HANDLE,
        UI::{
            Input::Pointer::{
                POINTER_CHANGE_FIRSTBUTTON_DOWN, POINTER_FLAG_DOWN, POINTER_FLAG_FIRSTBUTTON,
                POINTER_FLAG_INCONTACT, POINTER_FLAG_PRIMARY, POINTER_FLAG_UP, POINTER_FLAGS,
            },
            WindowsAndMessaging::{
                PEN_FLAG_ERASER, PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X,
                PEN_MASK_TILT_Y, PT_PEN, PT_TOUCH, TOUCH_MASK_CONTACTAREA, TOUCH_MASK_PRESSURE,
                WM_POINTERDOWN, WM_POINTERENTER, WM_POINTERUP, WM_POINTERUPDATE,
            },
        },
    };

    fn info() -> POINTER_INFO {
        POINTER_INFO {
            pointerId: 7,
            sourceDevice: HANDLE(0x3450_usize as *mut core::ffi::c_void),
            pointerFlags: POINTER_FLAG_PRIMARY,
            ptPixelLocationRaw: POINT { x: 120, y: 220 },
            ptPixelLocation: POINT { x: 900, y: 800 },
            ..POINTER_INFO::default()
        }
    }

    fn registry() -> ContactRegistry {
        ContactRegistry::new(MessageClock::anchored(0, None, 0).native())
            .with_reserved_contact(MOUSE_CONTACT)
    }

    /// `packet` assembled as `message` at client offset (-100, -200) and
    /// scale two.
    fn assemble(
        registry: &mut ContactRegistry,
        packets: &[PointerPacket],
        message: u32,
    ) -> Vec<PointerEvent> {
        assemble_history(
            registry,
            packets,
            message,
            POINT { x: -100, y: -200 },
            2.0,
            0,
            KeyboardModifiers::NONE,
        )
        .into_iter()
        .filter_map(|input| input.as_pointer().cloned())
        .collect()
    }

    /// The sample of the move `packet` makes for a hovering contact.
    fn sampled(packet: PointerPacket) -> (flui_platform::pointer::PointerInfo, PointerSample) {
        let events = assemble(&mut registry(), &[packet], WM_POINTERUPDATE);
        events
            .into_iter()
            .find_map(|event| match event {
                PointerEvent::Move(movement) => Some((movement.pointer, *movement.current())),
                _ => None,
            })
            .expect("a valid native reading is delivered")
    }

    fn native_raw_position_and_identity_survive() {
        let (pointer, sample) = sampled(PointerPacket::Pointer(info()));
        assert_eq!(
            sample.position.get(),
            flui_foundation::geometry::Point::new(10.0, 10.0)
        );
        assert!(pointer.device.is_some(), "the source device is minted");
        assert_eq!(pointer.role, PointerRole::Primary);
        assert_eq!(sample.pressure, None);
        let mut secondary = info();
        secondary.pointerFlags = POINTER_FLAGS::default();
        secondary.sourceDevice = HANDLE::default();
        let (pointer, _) = sampled(PointerPacket::Pointer(secondary));
        assert_eq!(pointer.role, PointerRole::Additional);
        assert_eq!(
            pointer.device, None,
            "missing hardware identity stays absent"
        );
    }

    fn pen(pen: POINTER_PEN_INFO) -> PointerPacket {
        PointerPacket::Pen(
            POINTER_PEN_INFO {
                pointerInfo: POINTER_INFO {
                    pointerType: PT_PEN,
                    ..info()
                },
                ..pen
            },
            None,
        )
    }

    /// A pen's himetric position, mapped through its device's rectangles,
    /// keeps the fraction of a pixel the integer pixel drops; a mapping that
    /// is empty, or that puts the pen more than a pixel from where the system
    /// says it is (a rotated display maps the axes otherwise), leaves the
    /// pixel.
    fn native_pen_position_keeps_himetric_precision() {
        let rect = |right, bottom| RECT {
            left: 0,
            top: 0,
            right,
            bottom,
        };
        // 10 himetric units to the pixel; `info()` is at pixel (120, 220),
        // read at client offset (-100, -200) and scale two.
        let tenth = HimetricMap {
            device: rect(10_000, 10_000),
            display: rect(1_000, 1_000),
        };
        let at = |himetric: POINT, map: Option<HimetricMap>| {
            let packet = POINTER_PEN_INFO {
                pointerInfo: POINTER_INFO {
                    pointerType: PT_PEN,
                    ptHimetricLocationRaw: himetric,
                    ..info()
                },
                ..Default::default()
            };
            let position = sampled(PointerPacket::Pen(packet, map)).1.position.get();
            (position.x, position.y)
        };
        let rows = [
            (
                "mapped",
                POINT { x: 1_205, y: 2_207 },
                Some(tenth),
                (10.25, 10.35),
            ),
            (
                "no mapping",
                POINT { x: 1_205, y: 2_207 },
                None,
                (10.0, 10.0),
            ),
            (
                "mapping disagrees with the pixel",
                POINT { x: 2_207, y: 1_205 },
                Some(tenth),
                (10.0, 10.0),
            ),
            (
                "empty device rectangle",
                POINT { x: 1_205, y: 2_207 },
                Some(HimetricMap {
                    device: rect(0, 10_000),
                    ..tenth
                }),
                (10.0, 10.0),
            ),
        ];
        for (name, himetric, map, (x, y)) in rows {
            let (got_x, got_y) = at(himetric, map);
            assert!(
                (got_x - x).abs() < 1e-9 && (got_y - y).abs() < 1e-9,
                "{name}: ({got_x}, {got_y}), expected ({x}, {y})"
            );
        }
    }

    fn native_pen_masks_preserve_sensor_presence() {
        let base = POINTER_PEN_INFO {
            pressure: 1024,
            rotation: 180,
            tiltX: 45,
            tiltY: 0,
            ..Default::default()
        };
        let (pointer, missing) = sampled(pen(base));
        assert_eq!(pointer.kind, PointerKind::Pen { tool: PenTool::Tip });
        assert_eq!(missing.pressure, None);
        assert_eq!(missing.orientation, None);
        assert_eq!(missing.twist, None);
        let (pointer, present) = sampled(pen(POINTER_PEN_INFO {
            penFlags: PEN_FLAG_ERASER,
            penMask: PEN_MASK_PRESSURE | PEN_MASK_ROTATION | PEN_MASK_TILT_X | PEN_MASK_TILT_Y,
            ..base
        }));
        assert_eq!(
            pointer.kind,
            PointerKind::Pen {
                tool: PenTool::Eraser
            }
        );
        assert_eq!(present.pressure.map(Pressure::get), Some(1.0));
        let orientation = present.orientation.expect("both native tilt axes");
        assert!(
            (orientation.altitude().expect("altitude") - std::f64::consts::FRAC_PI_4).abs() < 1e-12
        );
        assert_eq!(orientation.azimuth(), Some(0.0));
        assert!(
            (present.twist.expect("reported rotation").radians() - std::f64::consts::PI).abs()
                < 1e-12
        );
        let (_, partial) = sampled(pen(POINTER_PEN_INFO {
            penMask: PEN_MASK_TILT_X,
            ..base
        }));
        assert_eq!(
            partial.orientation, None,
            "one tilt axis does not determine a pen orientation"
        );
    }

    fn native_touch_contact_and_pressure_are_measured() {
        let touch = POINTER_TOUCH_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_TOUCH,
                pointerFlags: POINTER_FLAG_PRIMARY
                    | POINTER_FLAG_DOWN
                    | POINTER_FLAG_FIRSTBUTTON
                    | POINTER_FLAG_INCONTACT,
                ..info()
            },
            touchMask: TOUCH_MASK_CONTACTAREA | TOUCH_MASK_PRESSURE,
            rcContactRaw: windows::Win32::Foundation::RECT {
                left: 100,
                top: 200,
                right: 140,
                bottom: 260,
            },
            rcContact: windows::Win32::Foundation::RECT {
                left: 100,
                top: 200,
                right: 180,
                bottom: 320,
            },
            pressure: 512,
            ..Default::default()
        };
        let down = |touch| {
            assemble(
                &mut registry(),
                &[PointerPacket::Touch(touch)],
                WM_POINTERDOWN,
            )
            .into_iter()
            .find_map(|event| match event {
                PointerEvent::Down(press) => Some(press),
                _ => None,
            })
            .expect("touch down")
        };
        let press = down(touch);
        assert_eq!(press.pointer.kind, PointerKind::Touch);
        assert_eq!(press.sample.pressure.map(Pressure::get), Some(0.5));
        assert_eq!(
            press.sample.contact_size.expect("reported contact").get(),
            flui_foundation::geometry::Size::new(20.0, 30.0)
        );
        let absent = down(POINTER_TOUCH_INFO {
            touchMask: 0,
            ..touch
        });
        assert_eq!(absent.sample.pressure, None);
        assert_eq!(absent.sample.contact_size, None);
    }

    fn invalid_native_sensor_and_scale_are_refused() {
        for (pressure, scale) in [(1025, 1.0), (0, 0.0), (0, f64::NAN)] {
            let packet = pen(POINTER_PEN_INFO {
                penMask: PEN_MASK_PRESSURE,
                pressure,
                ..Default::default()
            });
            let reading = pointer_reading(
                packet,
                WM_POINTERUPDATE,
                POINT::default(),
                scale,
                0,
                KeyboardModifiers::NONE,
            );
            assert!(decode(&mut registry(), &reading).is_err());
        }
        let (_, sample) = sampled(pen(POINTER_PEN_INFO {
            penMask: PEN_MASK_PRESSURE,
            pressure: 0,
            ..Default::default()
        }));
        assert_eq!(
            sample.pressure.map(Pressure::get),
            Some(0.0),
            "reported zero is a sensor reading"
        );
    }

    fn touch_packet(id: u32, flags: POINTER_FLAGS) -> PointerPacket {
        PointerPacket::Touch(POINTER_TOUCH_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_TOUCH,
                pointerId: id,
                pointerFlags: flags,
                ..info()
            },
            ..Default::default()
        })
    }

    fn down(registry: &mut ContactRegistry, id: u32) -> flui_platform::pointer::PointerInfo {
        assemble(
            registry,
            &[touch_packet(
                id,
                POINTER_FLAG_DOWN | POINTER_FLAG_FIRSTBUTTON | POINTER_FLAG_INCONTACT,
            )],
            WM_POINTERDOWN,
        )
        .into_iter()
        .find_map(|event| match event {
            PointerEvent::Down(press) => Some(press.pointer),
            _ => None,
        })
        .expect("native contact must be admitted")
    }

    fn native_terminal_identity_survives_callback_readmission() {
        let mut registry = registry();
        let old = down(&mut registry, 7);
        let terminal = assemble(
            &mut registry,
            &[touch_packet(7, POINTER_FLAG_UP)],
            WM_POINTERUP,
        );
        // Presentation delivery can re-admit the native ID after retirement.
        let fresh = down(&mut registry, 7);
        assert_ne!(old.id, fresh.id);
        assert!(
            terminal
                .iter()
                .any(|event| matches!(event, PointerEvent::Up(release) if release.pointer == old))
        );
        let cancelled = registry.cancel_contact(
            SURFACE,
            NativeContact(7),
            EventTime::from_nanos(4),
            NativeCancel::Platform,
        );
        assert!(cancelled.iter().any(|input| matches!(input, PlatformInput::Pointer(PointerEvent::Cancel(cancel)) if cancel.pointer == fresh)));
    }

    fn enter_after_native_down_is_not_another_button_press() {
        let mut registry = registry();
        let native = PointerPacket::Pointer(POINTER_INFO {
            pointerFlags: POINTER_FLAG_DOWN | POINTER_FLAG_FIRSTBUTTON | POINTER_FLAG_INCONTACT,
            ButtonChangeType: POINTER_CHANGE_FIRSTBUTTON_DOWN,
            ..info()
        });
        let admitted = assemble(&mut registry, &[native], WM_POINTERDOWN);
        let entered = assemble(&mut registry, &[native], WM_POINTERENTER);
        assert!(
            admitted
                .iter()
                .any(|event| matches!(event, PointerEvent::Down(_)))
        );
        assert!(
            matches!(entered.as_slice(), [PointerEvent::Enter(_)]),
            "the native DOWN packet also backs the following ENTER notification: {entered:?}"
        );
    }

    fn native_enter_before_down_preserves_first_contact_admission() {
        let mut registry = registry();
        let native = PointerPacket::Pointer(POINTER_INFO {
            pointerFlags: POINTER_FLAG_DOWN | POINTER_FLAG_FIRSTBUTTON | POINTER_FLAG_INCONTACT,
            ButtonChangeType: POINTER_CHANGE_FIRSTBUTTON_DOWN,
            ..info()
        });
        let entered = assemble(&mut registry, &[native], WM_POINTERENTER);
        let enter_info = entered
            .iter()
            .find_map(|event| match event {
                PointerEvent::Enter(signal) => Some(signal.pointer),
                _ => None,
            })
            .expect("native entry");
        let contact = assemble(&mut registry, &[native], WM_POINTERDOWN);
        assert!(
            matches!(contact.as_slice(), [PointerEvent::Down(press)] if press.pointer == enter_info),
            "ENTER observes the identity but does not admit its first press: {contact:?}"
        );
    }

    /// 100 ns counter ticks anchored at uptime 5000 ms and epoch 1 s.
    fn performance_clock() -> MessageClock {
        MessageClock::anchored(
            5_000,
            Some(CounterAnchor {
                count: QpcTicks(1_000_000),
                frequency: std::num::NonZeroU64::new(10_000_000).expect("nonzero"),
            }),
            1_000_000_000,
        )
    }

    fn timed_touch(dw_time: u32, count: u64, x: i32) -> PointerPacket {
        PointerPacket::Touch(POINTER_TOUCH_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_TOUCH,
                pointerFlags: POINTER_FLAG_PRIMARY
                    | POINTER_FLAG_FIRSTBUTTON
                    | POINTER_FLAG_INCONTACT,
                dwTime: dw_time,
                PerformanceCount: count,
                ptPixelLocationRaw: POINT { x, y: 220 },
                ..info()
            },
            ..Default::default()
        })
    }

    /// A 240 Hz burst inside one system tick: every reading carries the same
    /// `dwTime`, and only the performance count tells them apart. Native
    /// history is newest first; the contact went down at the burst's start.
    fn assembled_burst(clock: &MessageClock) -> flui_platform::pointer::PointerMove {
        let mut registry = ContactRegistry::new(clock.native());
        let start = timed_touch(5_016, 1_160_000, 130);
        assemble_history(
            &mut registry,
            &[start],
            WM_POINTERDOWN,
            POINT { x: -100, y: -200 },
            2.0,
            5_020,
            KeyboardModifiers::NONE,
        );
        let burst = [
            timed_touch(5_016, 1_285_001, 160),
            timed_touch(5_016, 1_243_334, 150),
            timed_touch(5_016, 1_201_667, 140),
            timed_touch(5_016, 1_160_000, 130),
        ];
        let events = assemble_history(
            &mut registry,
            &burst,
            WM_POINTERUPDATE,
            POINT { x: -100, y: -200 },
            2.0,
            5_020,
            KeyboardModifiers::NONE,
        );
        match events.as_slice() {
            [PlatformInput::Pointer(PointerEvent::Move(movement))] => movement.clone(),
            other => panic!("one batched move expected, got {other:?}"),
        }
    }

    fn coalesced_burst_is_ordered_by_performance_count() {
        let movement = assembled_burst(&performance_clock());
        assert_eq!(
            movement
                .coalesced()
                .iter()
                .map(|sample| sample.time.as_nanos())
                .collect::<Vec<_>>(),
            [1_016_000_000, 1_020_166_700, 1_024_333_400],
            "oldest first, at counter resolution"
        );
        assert_eq!(
            movement
                .coalesced()
                .iter()
                .map(|sample| sample.position.get().x)
                .collect::<Vec<_>>(),
            [15.0, 20.0, 25.0],
            "each time stays with its own reading"
        );
    }

    fn current_reading_is_stamped_from_its_performance_count() {
        let movement = assembled_burst(&performance_clock());
        assert_eq!(movement.current().time.as_nanos(), 1_028_500_100);
        assert_eq!(movement.current().position.get().x, 30.0);
    }

    fn reading_time(clock: &MessageClock, count: u64) -> u64 {
        sampled_on(clock, timed_touch(5_016, count, 0))
    }

    fn sampled_on(clock: &MessageClock, packet: PointerPacket) -> u64 {
        let mut registry = ContactRegistry::new(clock.native());
        let events = assemble_history(
            &mut registry,
            &[packet],
            WM_POINTERDOWN,
            POINT::default(),
            1.0,
            5_020,
            KeyboardModifiers::NONE,
        );
        events
            .iter()
            .find_map(|input| match input.as_pointer() {
                Some(PointerEvent::Down(press)) => Some(press.sample.time.as_nanos()),
                _ => None,
            })
            .expect("down")
    }

    fn reading_without_count_uses_system_tick() {
        assert_eq!(reading_time(&performance_clock(), 0), 1_016_000_000);
    }

    fn reading_without_counter_anchor_uses_system_tick() {
        let clock = MessageClock::anchored(5_000, None, 1_000_000_000);
        assert_eq!(reading_time(&clock, 1_160_000), 1_016_000_000);
    }

    fn count_before_the_epoch_precedes_it() {
        assert_eq!(reading_time(&performance_clock(), 990_000), 999_000_000);
    }

    fn count_past_the_epoch_range_saturates() {
        assert_eq!(reading_time(&performance_clock(), u64::MAX), u64::MAX);
    }

    #[test]
    fn native_pointer_decoding_contracts() {
        for case in [
            native_raw_position_and_identity_survive as fn(),
            native_pen_masks_preserve_sensor_presence,
            native_pen_position_keeps_himetric_precision,
            native_touch_contact_and_pressure_are_measured,
            invalid_native_sensor_and_scale_are_refused,
            native_terminal_identity_survives_callback_readmission,
            enter_after_native_down_is_not_another_button_press,
            native_enter_before_down_preserves_first_contact_admission,
            coalesced_burst_is_ordered_by_performance_count,
            current_reading_is_stamped_from_its_performance_count,
            reading_without_count_uses_system_tick,
            reading_without_counter_anchor_uses_system_tick,
            count_before_the_epoch_precedes_it,
            count_past_the_epoch_range_saturates,
        ] {
            case();
        }
    }
}
#[cfg(test)]
mod mouse_button_codes {
    use windows::Win32::System::SystemServices::{
        MK_LBUTTON, MK_MBUTTON, MK_RBUTTON, MK_XBUTTON1, MK_XBUTTON2,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
    };

    use super::super::MOUSE_BUTTONS;

    /// The key model's mouse buttons, written out as integers so every host
    /// builds them, carry Windows' own virtual keys and `MK_*` bits.
    #[test]
    fn the_mouse_buttons_carry_the_win32_codes() {
        let win32 = [
            (VK_LBUTTON, MK_LBUTTON),
            (VK_RBUTTON, MK_RBUTTON),
            (VK_MBUTTON, MK_MBUTTON),
            (VK_XBUTTON1, MK_XBUTTON1),
            (VK_XBUTTON2, MK_XBUTTON2),
        ];
        for (codes, (vk, mk)) in MOUSE_BUTTONS.into_iter().zip(win32) {
            assert_eq!(
                (codes.virtual_key, codes.key_state_bit),
                (vk.0, mk.0),
                "{:?}",
                codes.button
            );
        }
    }
}
