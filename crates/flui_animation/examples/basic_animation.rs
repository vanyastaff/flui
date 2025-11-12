//! Basic animation example demonstrating core animation concepts.

use flui_animation::prelude::*;
use flui_scheduler::Scheduler;
use flui_types::animation::{ColorTween, FloatTween};
use flui_types::styling::Color;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    println!("FLUI Animation Example\n");

    // Create a scheduler
    let scheduler = Arc::new(Scheduler::new());

    // 1. Basic AnimationController
    println!("1. AnimationController:");
    let controller = AnimationController::new(Duration::from_millis(300), scheduler.clone());

    controller.set_value(0.0);
    println!("   Initial value: {}", controller.value());

    controller.set_value(0.5);
    println!("   Mid value: {}", controller.value());

    controller.set_value(1.0);
    println!("   Final value: {}", controller.value());

    // 2. CurvedAnimation
    println!("\n2. CurvedAnimation (Ease In/Out):");
    let curved = CurvedAnimation::new(
        Arc::new(controller.clone()) as Arc<dyn Animation<f32>>,
        Curves::EaseInOut,
    );

    controller.set_value(0.0);
    println!("   t=0.0 → {:.3}", curved.value());

    controller.set_value(0.5);
    println!("   t=0.5 → {:.3}", curved.value());

    controller.set_value(1.0);
    println!("   t=1.0 → {:.3}", curved.value());

    // 3. TweenAnimation
    println!("\n3. TweenAnimation (0.0 → 100.0):");
    let tween = FloatTween::new(0.0, 100.0);
    let tween_anim = TweenAnimation::new(
        tween,
        Arc::new(controller.clone()) as Arc<dyn Animation<f32>>,
    );

    controller.set_value(0.0);
    println!("   t=0.0 → {:.1}", tween_anim.value());

    controller.set_value(0.5);
    println!("   t=0.5 → {:.1}", tween_anim.value());

    controller.set_value(1.0);
    println!("   t=1.0 → {:.1}", tween_anim.value());

    // 4. ReverseAnimation
    println!("\n4. ReverseAnimation:");
    let reversed = ReverseAnimation::new(Arc::new(controller.clone()) as Arc<dyn Animation<f32>>);

    controller.set_value(0.0);
    println!("   parent=0.0 → reversed={:.1}", reversed.value());

    controller.set_value(0.5);
    println!("   parent=0.5 → reversed={:.1}", reversed.value());

    controller.set_value(1.0);
    println!("   parent=1.0 → reversed={:.1}", reversed.value());

    // 5. CompoundAnimation
    println!("\n5. CompoundAnimation (Addition):");
    let controller2 = AnimationController::new(Duration::from_millis(300), scheduler);
    controller2.set_value(0.3);

    let compound = CompoundAnimation::add(
        Arc::new(controller.clone()) as Arc<dyn Animation<f32>>,
        Arc::new(controller2.clone()) as Arc<dyn Animation<f32>>,
    );

    controller.set_value(0.5);
    println!("   0.5 + 0.3 = {:.1}", compound.value());

    controller.set_value(0.7);
    println!("   0.7 + 0.3 = {:.1}", compound.value());

    // 6. ProxyAnimation
    println!("\n6. ProxyAnimation (hot-swapping):");
    let proxy = ProxyAnimation::new(Arc::new(controller.clone()) as Arc<dyn Animation<f32>>);

    controller.set_value(0.5);
    println!("   Using controller: {:.1}", proxy.value());

    proxy.set_parent(Arc::new(controller2.clone()) as Arc<dyn Animation<f32>>);
    println!("   Swapped to controller2: {:.1}", proxy.value());

    // 7. Complex composition
    println!("\n7. Complex Composition:");
    println!("   Creating: Tween(ColorTween) + Curved(Elastic) + Controller");

    let color_tween = ColorTween::new(Color::RED, Color::BLUE);
    let elastic_curved = CurvedAnimation::new(
        Arc::new(controller.clone()) as Arc<dyn Animation<f32>>,
        Curves::ElasticOut,
    );
    let color_anim = TweenAnimation::new(
        color_tween,
        Arc::new(elastic_curved) as Arc<dyn Animation<f32>>,
    );

    controller.set_value(0.0);
    let c0 = color_anim.value();
    println!("   t=0.0 → Color(r={}, g={}, b={})", c0.r, c0.g, c0.b);

    controller.set_value(1.0);
    let c1 = color_anim.value();
    println!("   t=1.0 → Color(r={}, g={}, b={})", c1.r, c1.g, c1.b);

    // Cleanup
    controller.dispose();
    controller2.dispose();

    println!("\n✓ All examples completed successfully!");
}
