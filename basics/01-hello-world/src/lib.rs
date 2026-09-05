//! # 01 - Hello World Plugin Example for Pumpkin
//!
//! This is the simplest possible Pumpkin plugin. It demonstrates:
//! - Defining a plugin struct and implementing the [`Plugin`] trait.
//! - Setting up metadata (name, version, authors, permissions).
//! - Handling the [`on_load`] and [`on_unload`] lifecycle callbacks.
//! - Logging using the `tracing` crate, which connects to Pumpkin's server log system.
//! - Registering the plugin with the [`register_plugin!`] macro.

use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, permissions, register_plugin,
};

/// The main plugin state struct.
pub struct HelloWorldPlugin;

impl Plugin for HelloWorldPlugin {
    /// Constructs a new instance of the plugin.
    ///
    /// Called once by the runtime before [`on_load`](Plugin::on_load).
    fn new() -> Self {
        HelloWorldPlugin
    }

    /// Provides metadata describing this plugin to the Pumpkin server.
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "hello-world".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Pumpkin Developer".into()],
            description: "A minimal Hello World plugin for Pumpkin Minecraft server.".into(),
            dependencies: vec![],
            permissions: vec![permissions::FS_READ_DATA.into()],
        }
    }

    /// Invoked when the server loads and enables the plugin.
    fn on_load(&self, context: Context) -> Result<(), String> {
        tracing::info!("Hello, world from Pumpkin plugin!");
        tracing::info!("Plugin private data directory: {}", context.get_data_folder());
        Ok(())
    }

    /// Invoked when the server unloads or stops the plugin.
    fn on_unload(&self, _context: Context) -> Result<(), String> {
        tracing::info!("Goodbye from Hello World plugin!");
        Ok(())
    }
}

// Register HelloWorldPlugin as the entrypoint for the WebAssembly component.
register_plugin!(HelloWorldPlugin);
