//! The crate's `[package.metadata.flui.capability]`, which `flui add system`
//! reads to print the line that registers the plugin (ADR-0307 §5).

use flui_platform::capability::Plugin;
use flui_system::SystemPlugin;

/// The `plugin` the manifest declares, by a plain read of `Cargo.toml`.
fn declared_plugin() -> String {
    let manifest: toml::Table = include_str!("../Cargo.toml")
        .parse()
        .expect("the crate's Cargo.toml");
    manifest["package"]["metadata"]["flui"]["capability"]["plugin"]
        .as_str()
        .expect("a plugin path")
        .to_owned()
}

/// The declared path names the plugin type from the crate root, and the type
/// is a `Plugin` with a `Default`, so the line `flui add` prints compiles.
/// Renaming or moving the type, or editing the declaration alone, fails here.
#[test]
fn the_declared_plugin_is_the_crates_default_plugin() {
    fn registrable<P: Plugin + Default>() -> P {
        P::default()
    }
    let _plugin: SystemPlugin = registrable();
    assert_eq!(
        std::any::type_name::<SystemPlugin>(),
        format!("flui_system::{}", declared_plugin())
    );
}
