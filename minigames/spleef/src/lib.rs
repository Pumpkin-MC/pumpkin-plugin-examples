//! # Spleef Minigame for Pumpkin
//!
//! A complete, runnable Spleef minigame showcasing:
//! - State management (`Waiting`, `Active`, `Ended`).
//! - Commands: `/spleef join`, `/spleef leave`, `/spleef start`, `/spleef status`, `/spleef reset`.
//! - [`BlockBreakEvent`]:
//!   - Allows breaking snow blocks inside the active arena.
//!   - Protects blocks outside the arena boundary or before the game starts.
//! - [`PlayerMoveEvent`]:
//!   - Detects players falling below the snow platform into the elimination zone.
//! - Automatic win condition check (last player standing).
//! - Arena regeneration (`/spleef reset`).

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use pumpkin_plugin_api::{
    command::{Command, CommandError, CommandNode, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    common::{BlockPos, Position},
    events::{
        EventHandler, EventPriority,
        block::BlockBreakEvent,
        player::PlayerMoveEvent,
    },
    events_wit::{BlockBreakEventData, PlayerMoveEventData},
    world::BlockFlags,
    Context, Plugin, PluginMetadata, register_plugin, Server,
    text::TextComponent,
};

/// Spleef arena boundaries and platform heights.
#[derive(Clone, Debug)]
pub struct SpleefConfig {
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
    pub floor_y: i32,
    pub elimination_y: f64,
    pub spawn_pos: Position,
    pub lobby_pos: Position,
}

impl Default for SpleefConfig {
    fn default() -> Self {
        Self {
            min_x: -12,
            max_x: 12,
            min_z: -12,
            max_z: 12,
            floor_y: 100,
            elimination_y: 92.0,
            spawn_pos: (0.5, 101.0, 0.5),
            lobby_pos: (0.5, 115.0, 0.5),
        }
    }
}

impl SpleefConfig {
    pub fn is_inside_floor(&self, x: i32, y: i32, z: i32) -> bool {
        y == self.floor_y && x >= self.min_x && x <= self.max_x && z >= self.min_z && z <= self.max_z
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameState {
    Waiting,
    Active,
    Ended,
}

pub struct SpleefSession {
    pub state: GameState,
    pub config: SpleefConfig,
    pub queued_players: HashSet<String>,
    pub alive_players: HashSet<String>,
}

impl SpleefSession {
    pub fn new(config: SpleefConfig) -> Self {
        Self {
            state: GameState::Waiting,
            config,
            queued_players: HashSet::new(),
            alive_players: HashSet::new(),
        }
    }
}

static SESSION: Mutex<Option<Arc<Mutex<SpleefSession>>>> = Mutex::new(None);

fn get_session() -> Arc<Mutex<SpleefSession>> {
    SESSION
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .expect("SpleefSession must be initialized in on_load")
}

// -----------------------------------------------------------------------------
// Commands: /spleef <subcommand>
// -----------------------------------------------------------------------------

struct SpleefRootHandler;
impl CommandHandler for SpleefRootHandler {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let help_text = "Spleef Commands:\n\
            /spleef join   - Join the game queue\n\
            /spleef leave  - Leave the queue or game\n\
            /spleef start  - Force start the game\n\
            /spleef status - View game status\n\
            /spleef reset  - Regenerate snow arena floor";
        sender.send_message(TextComponent::text(help_text));
        Ok(0)
    }
}

struct SpleefJoinHandler;
impl CommandHandler for SpleefJoinHandler {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let player_name = sender.get_name();
        let session_arc = get_session();
        let mut session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        match session.state {
            GameState::Active => {
                sender.send_message(TextComponent::text("Game is already in progress! Please wait."));
            }
            GameState::Waiting | GameState::Ended => {
                if session.queued_players.insert(player_name.clone()) {
                    let count = session.queued_players.len();
                    server.broadcast(&format!("{player_name} joined Spleef! ({count} players queued)"));
                    sender.send_message(TextComponent::text("You joined the queue! Use /spleef start when ready."));
                } else {
                    sender.send_message(TextComponent::text("You are already in the queue!"));
                }
            }
        }
        Ok(0)
    }
}

struct SpleefLeaveHandler;
impl CommandHandler for SpleefLeaveHandler {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let player_name = sender.get_name();
        let session_arc = get_session();
        let mut session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        if session.queued_players.remove(&player_name) || session.alive_players.remove(&player_name) {
            server.broadcast(&format!("{player_name} left Spleef."));
            sender.send_message(TextComponent::text("You left Spleef."));
        } else {
            sender.send_message(TextComponent::text("You are not currently in a Spleef game."));
        }
        Ok(0)
    }
}

struct SpleefStartHandler;
impl CommandHandler for SpleefStartHandler {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let session_arc = get_session();
        let mut session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        if session.state == GameState::Active {
            sender.send_message(TextComponent::text("Game is already running!"));
            return Ok(0);
        }

        if session.queued_players.is_empty() {
            session.queued_players.insert(sender.get_name());
        }

        session.alive_players = session.queued_players.clone();
        session.state = GameState::Active;

        let count = session.alive_players.len();
        server.broadcast(&format!("Spleef has STARTED with {count} players! Spleef your opponents!"));

        let spawn = session.config.spawn_pos;
        for player_name in &session.alive_players {
            if let Some(player) = server.get_player_by_name(player_name) {
                let world = player.get_world();
                player.teleport(spawn, Some(0.0), Some(0.0), world);
                player.show_title(TextComponent::text("SPLEEF!"));
                player.show_subtitle(TextComponent::text("Break blocks beneath your rivals!"));
            }
        }

        Ok(0)
    }
}

struct SpleefStatusHandler;
impl CommandHandler for SpleefStatusHandler {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let session_arc = get_session();
        let session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        let status_msg = match session.state {
            GameState::Waiting => format!("Spleef: WAITING ({} queued)", session.queued_players.len()),
            GameState::Active => format!(
                "Spleef: ACTIVE ({} alive: {:?})",
                session.alive_players.len(),
                session.alive_players
            ),
            GameState::Ended => "Spleef: ENDED (Use /spleef reset or /spleef start)".to_string(),
        };

        sender.send_message(TextComponent::text(&status_msg));
        Ok(0)
    }
}

struct SpleefResetHandler;
impl CommandHandler for SpleefResetHandler {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let session_arc = get_session();
        let mut session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        session.state = GameState::Waiting;
        session.alive_players.clear();

        let worlds = server.get_all_worlds();
        if let Some(world) = worlds.first() {
            let flags = BlockFlags::NOTIFY_LISTENERS | BlockFlags::NOTIFY_NEIGHBORS;
            for x in session.config.min_x..=session.config.max_x {
                for z in session.config.min_z..=session.config.max_z {
                    let pos = BlockPos { x, y: session.config.floor_y, z };
                    world.set_block_by_name(pos, "minecraft:snow_block", flags);
                }
            }
        }

        server.broadcast("Spleef arena has been regenerated and reset!");
        sender.send_message(TextComponent::text("Spleef reset complete!"));
        Ok(0)
    }
}

// -----------------------------------------------------------------------------
// Event Listeners: BlockBreakEvent & PlayerMoveEvent
// -----------------------------------------------------------------------------

struct SpleefBreakListener;

impl EventHandler<BlockBreakEvent> for SpleefBreakListener {
    fn handle(&self, _server: Server, mut event: BlockBreakEventData) -> BlockBreakEventData {
        let session_arc = get_session();
        let session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        let pos = &event.block_pos;

        // If the block broken is on the Spleef floor:
        if session.config.is_inside_floor(pos.x, pos.y, pos.z) {
            // Only allow breaking during Active state by alive players
            if session.state == GameState::Active {
                if let Some(player) = &event.player {
                    if session.alive_players.contains(&player.get_name()) {
                        event.should_drop = false; // Don't drop cluttering snowballs
                        event.cancelled = false;
                        return event;
                    }
                }
            }
            // Disallow breaking floor before start or by spectators
            event.cancelled = true;
        }

        event
    }
}

struct SpleefMoveListener;

impl EventHandler<PlayerMoveEvent> for SpleefMoveListener {
    fn handle(&self, server: Server, event: PlayerMoveEventData) -> PlayerMoveEventData {
        let player_name = event.player.get_name();
        let session_arc = get_session();
        let mut session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        if session.state != GameState::Active || !session.alive_players.contains(&player_name) {
            return event;
        }

        let to = &event.to_position;

        // Elimination Check: player fell below elimination threshold
        if to.1 < session.config.elimination_y {
            session.alive_players.remove(&player_name);
            server.broadcast(&format!("{player_name} was SPLEEFED and eliminated!"));

            let lobby = session.config.lobby_pos;
            let world = event.player.get_world();
            event.player.teleport(lobby, Some(0.0), Some(0.0), world);
            event.player.show_title(TextComponent::text("SPLEEFED!"));

            if session.alive_players.len() == 1 {
                let winner_name = session.alive_players.iter().next().cloned().unwrap();
                server.broadcast(&format!("VICTORY! {winner_name} is the last player standing and WINS Spleef!"));
                if let Some(winner) = server.get_player_by_name(&winner_name) {
                    winner.show_title(TextComponent::text("VICTORY!"));
                    winner.show_subtitle(TextComponent::text("You won Spleef!"));
                }
                session.state = GameState::Ended;
            } else if session.alive_players.is_empty() {
                server.broadcast("Game Over! No players remaining.");
                session.state = GameState::Ended;
            }
        }

        event
    }
}

// -----------------------------------------------------------------------------
// Plugin Lifecycle
// -----------------------------------------------------------------------------

pub struct SpleefPlugin;

impl Plugin for SpleefPlugin {
    fn new() -> Self {
        SpleefPlugin
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "spleef".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Pumpkin Developer".into()],
            description: "A complete Spleef minigame for Pumpkin Minecraft server.".into(),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let config = SpleefConfig::default();
        let session = Arc::new(Mutex::new(SpleefSession::new(config)));
        *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);

        let names = ["spleef".to_string(), "sp".to_string()];
        let command = Command::new(&names, "Spleef minigame management command")
            .then(CommandNode::literal("join").execute(SpleefJoinHandler))
            .then(CommandNode::literal("leave").execute(SpleefLeaveHandler))
            .then(CommandNode::literal("start").execute(SpleefStartHandler))
            .then(CommandNode::literal("status").execute(SpleefStatusHandler))
            .then(CommandNode::literal("reset").execute(SpleefResetHandler))
            .execute(SpleefRootHandler);

        context.register_command(command, "pumpkin.command.spleef");

        // Register BlockBreakEvent (blocking = true to allow/cancel break)
        context.register_event_handler(
            SpleefBreakListener,
            EventPriority::High,
            true,
        )?;

        // Register PlayerMoveEvent for void detection
        context.register_event_handler(
            SpleefMoveListener,
            EventPriority::Normal,
            false,
        )?;

        tracing::info!("Spleef minigame plugin successfully loaded!");
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<(), String> {
        tracing::info!("Spleef plugin unloaded.");
        Ok(())
    }
}

register_plugin!(SpleefPlugin);
