//! # 03 - Event Listener Plugin Example for Pumpkin
//!
//! Demonstrates:
//! - Subscribing to Pumpkin server events using [`EventHandler`].
//! - Registering listeners with [`Context::register_event_handler`].
//! - Setting [`EventPriority`] and configuring blocking vs non-blocking handlers.
//! - Handling player join and leave lifecycle events ([`PlayerJoinEvent`], [`PlayerLeaveEvent`]).
//! - Interacting with players (custom messages, titles, action bar).
//! - Handling block events ([`BlockBreakEvent`]) and cancelling events (`cancelled = true`) to protect blocks.

use pumpkin_plugin_api::{
    events::{
        EventHandler, EventPriority,
        block::BlockBreakEvent,
        player::{PlayerJoinEvent, PlayerLeaveEvent},
    },
    events_wit::{BlockBreakEventData, PlayerJoinEventData, PlayerLeaveEventData},
    Context, Plugin, PluginMetadata, register_plugin, Server,
    text::TextComponent,
};

/// Listens for players joining the server.
struct JoinListener;

impl EventHandler<PlayerJoinEvent> for JoinListener {
    fn handle(&self, _server: Server, event: PlayerJoinEventData) -> PlayerJoinEventData {
        let name = event.player.get_name();
        tracing::info!("Player joined the game: {name}");

        // Send a private chat message to the player
        event.player.send_system_message(
            TextComponent::text(&format!("Welcome to the server, {name}!")),
            false,
        );

        // Show a welcoming title screen to the player
        event
            .player
            .show_title(TextComponent::text("Welcome to Pumpkin!"));
        event
            .player
            .show_subtitle(TextComponent::text("Have fun building and exploring!"));

        event
    }
}

/// Listens for players leaving the server.
struct LeaveListener;

impl EventHandler<PlayerLeaveEvent> for LeaveListener {
    fn handle(&self, _server: Server, event: PlayerLeaveEventData) -> PlayerLeaveEventData {
        let name = event.player.get_name();
        tracing::info!("Player left the game: {name}");
        event
    }
}

/// Listens for block breaking events and demonstrates event cancellation.
struct BlockBreakListener;

impl EventHandler<BlockBreakEvent> for BlockBreakListener {
    fn handle(&self, _server: Server, mut event: BlockBreakEventData) -> BlockBreakEventData {
        let block_name = &event.block;
        let pos = &event.block_pos;

        tracing::info!(
            "Block broken: {block_name} at X:{} Y:{} Z:{}",
            pos.x, pos.y, pos.z
        );

        // Example: Protect bedrock and barrier blocks from being broken
        if block_name.contains("bedrock") || block_name.contains("barrier") {
            tracing::warn!("Cancelled breaking protected block '{block_name}' at ({}, {}, {})", pos.x, pos.y, pos.z);
            event.cancelled = true;

            if let Some(player) = &event.player {
                player.send_system_message(
                    TextComponent::text("You cannot break this protected block!"),
                    true, // overlay = true displays in the action bar
                );
            }
        }

        event
    }
}

/// Main plugin struct for the event listener example.
pub struct EventListenerPlugin;

impl Plugin for EventListenerPlugin {
    fn new() -> Self {
        EventListenerPlugin
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "event-listener".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Pumpkin Developer".into()],
            description: "An event listener example for the Pumpkin Minecraft server.".into(),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        // Register PlayerJoinEvent (non-blocking notification)
        context.register_event_handler(
            JoinListener,
            EventPriority::Normal,
            false,
        )?;

        // Register PlayerLeaveEvent (non-blocking notification)
        context.register_event_handler(
            LeaveListener,
            EventPriority::Normal,
            false,
        )?;

        // Register BlockBreakEvent (blocking = true allows event.cancelled to take effect)
        context.register_event_handler(
            BlockBreakListener,
            EventPriority::High,
            true,
        )?;

        tracing::info!("Registered player join, player leave, and block break event handlers!");
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<(), String> {
        tracing::info!("Unloaded event listener plugin!");
        Ok(())
    }
}

register_plugin!(EventListenerPlugin);
