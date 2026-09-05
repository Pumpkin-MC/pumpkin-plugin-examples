//! # Sumo Minigame for Pumpkin
//!
//! A complete, runnable Sumo PvP minigame showcasing:
//! - State management (`Waiting`, `Active`, `Ended`).
//! - Commands: `/sumo join`, `/sumo leave`, `/sumo start`, `/sumo status`, `/sumo reset`.
//! - Arena protection: cancels all block breaking on the Sumo ring via [`BlockBreakEvent`].
//! - Ring-out elimination: monitors [`PlayerMoveEvent`] to detect when a player is knocked off.
//! - Automatic win condition check (last player remaining on the platform).
//! - Platform regeneration (`/sumo reset`).

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

/// Sumo ring boundaries and platform coordinates.
#[derive(Clone, Debug)]
pub struct SumoConfig {
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
    pub platform_y: i32,
    pub elimination_y: f64,
    pub spawn_pos_1: Position,
    pub spawn_pos_2: Position,
    pub lobby_pos: Position,
}

impl Default for SumoConfig {
    fn default() -> Self {
        Self {
            min_x: -6,
            max_x: 6,
            min_z: -6,
            max_z: 6,
            platform_y: 100,
            elimination_y: 95.0,
            spawn_pos_1: (-3.5, 101.0, 0.5),
            spawn_pos_2: (3.5, 101.0, 0.5),
            lobby_pos: (0.5, 115.0, 0.5),
        }
    }
}

impl SumoConfig {
    pub fn is_inside_platform(&self, x: i32, y: i32, z: i32) -> bool {
        y == self.platform_y && x >= self.min_x && x <= self.max_x && z >= self.min_z && z <= self.max_z
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameState {
    Waiting,
    Active,
    Ended,
}

pub struct SumoSession {
    pub state: GameState,
    pub config: SumoConfig,
    pub queued_players: HashSet<String>,
    pub alive_players: HashSet<String>,
}

impl SumoSession {
    pub fn new(config: SumoConfig) -> Self {
        Self {
            state: GameState::Waiting,
            config,
            queued_players: HashSet::new(),
            alive_players: HashSet::new(),
        }
    }
}

static SESSION: Mutex<Option<Arc<Mutex<SumoSession>>>> = Mutex::new(None);

fn get_session() -> Arc<Mutex<SumoSession>> {
    SESSION
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .expect("SumoSession must be initialized in on_load")
}

// -----------------------------------------------------------------------------
// Commands: /sumo <subcommand>
// -----------------------------------------------------------------------------

struct SumoRootHandler;
impl CommandHandler for SumoRootHandler {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let help_text = "Sumo Commands:\n\
            /sumo join   - Join the Sumo queue\n\
            /sumo leave  - Leave the queue or game\n\
            /sumo start  - Force start the duel\n\
            /sumo status - View game status\n\
            /sumo reset  - Regenerate arena ring";
        sender.send_message(TextComponent::text(help_text));
        Ok(0)
    }
}

struct SumoJoinHandler;
impl CommandHandler for SumoJoinHandler {
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
                sender.send_message(TextComponent::text("A Sumo round is currently active! Please wait."));
            }
            GameState::Waiting | GameState::Ended => {
                if session.queued_players.insert(player_name.clone()) {
                    let count = session.queued_players.len();
                    server.broadcast(&format!("{player_name} joined Sumo! ({count} fighters queued)"));
                    sender.send_message(TextComponent::text("You joined the queue! Use /sumo start when ready."));
                } else {
                    sender.send_message(TextComponent::text("You are already in the queue!"));
                }
            }
        }
        Ok(0)
    }
}

struct SumoLeaveHandler;
impl CommandHandler for SumoLeaveHandler {
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
            server.broadcast(&format!("{player_name} left Sumo."));
            sender.send_message(TextComponent::text("You left Sumo."));
        } else {
            sender.send_message(TextComponent::text("You are not currently in a Sumo game."));
        }
        Ok(0)
    }
}

struct SumoStartHandler;
impl CommandHandler for SumoStartHandler {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let session_arc = get_session();
        let mut session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        if session.state == GameState::Active {
            sender.send_message(TextComponent::text("Sumo match is already running!"));
            return Ok(0);
        }

        if session.queued_players.is_empty() {
            session.queued_players.insert(sender.get_name());
        }

        session.alive_players = session.queued_players.clone();
        session.state = GameState::Active;

        let count = session.alive_players.len();
        server.broadcast(&format!("Sumo duel has STARTED with {count} fighters! Knock your opponents off the ring!"));

        // Alternate spawn positions for players facing each other
        let spawns = [session.config.spawn_pos_1, session.config.spawn_pos_2];
        for (idx, player_name) in session.alive_players.iter().enumerate() {
            if let Some(player) = server.get_player_by_name(player_name) {
                let world = player.get_world();
                let spawn = spawns[idx % spawns.len()];
                player.teleport(spawn, Some(0.0), Some(0.0), world);
                player.show_title(TextComponent::text("SUMO!"));
                player.show_subtitle(TextComponent::text("Knock the other players off the ring!"));
            }
        }

        Ok(0)
    }
}

struct SumoStatusHandler;
impl CommandHandler for SumoStatusHandler {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let session_arc = get_session();
        let session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        let status_msg = match session.state {
            GameState::Waiting => format!("Sumo: WAITING ({} fighters queued)", session.queued_players.len()),
            GameState::Active => format!(
                "Sumo: ACTIVE ({} fighters on ring: {:?})",
                session.alive_players.len(),
                session.alive_players
            ),
            GameState::Ended => "Sumo: ENDED (Use /sumo reset or /sumo start)".to_string(),
        };

        sender.send_message(TextComponent::text(&status_msg));
        Ok(0)
    }
}

struct SumoResetHandler;
impl CommandHandler for SumoResetHandler {
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
                    let pos = BlockPos { x, y: session.config.platform_y, z };
                    world.set_block_by_name(pos, "minecraft:smooth_stone", flags);
                }
            }
        }

        server.broadcast("Sumo ring has been regenerated and reset!");
        sender.send_message(TextComponent::text("Sumo reset complete!"));
        Ok(0)
    }
}

// -----------------------------------------------------------------------------
// Event Listeners: BlockBreakEvent & PlayerMoveEvent
// -----------------------------------------------------------------------------

struct SumoBreakListener;

impl EventHandler<BlockBreakEvent> for SumoBreakListener {
    fn handle(&self, _server: Server, mut event: BlockBreakEventData) -> BlockBreakEventData {
        let session_arc = get_session();
        let session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        let pos = &event.block_pos;

        // The Sumo arena platform is strictly unbreakable
        if session.config.is_inside_platform(pos.x, pos.y, pos.z) {
            event.cancelled = true;
            if let Some(player) = &event.player {
                player.send_system_message(
                    TextComponent::text("You cannot break the Sumo ring!"),
                    true,
                );
            }
        }

        event
    }
}

struct SumoMoveListener;

impl EventHandler<PlayerMoveEvent> for SumoMoveListener {
    fn handle(&self, server: Server, event: PlayerMoveEventData) -> PlayerMoveEventData {
        let player_name = event.player.get_name();
        let session_arc = get_session();
        let mut session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        if session.state != GameState::Active || !session.alive_players.contains(&player_name) {
            return event;
        }

        let to = &event.to_position;

        // Ring-out check: knocked off the platform below elimination Y
        if to.1 < session.config.elimination_y {
            session.alive_players.remove(&player_name);
            server.broadcast(&format!("{player_name} was KNOCKED OFF the ring and eliminated!"));

            let lobby = session.config.lobby_pos;
            let world = event.player.get_world();
            event.player.teleport(lobby, Some(0.0), Some(0.0), world);
            event.player.show_title(TextComponent::text("RING OUT!"));

            if session.alive_players.len() == 1 {
                let winner_name = session.alive_players.iter().next().cloned().unwrap();
                server.broadcast(&format!("VICTORY! {winner_name} pushed everyone off and WINS Sumo!"));
                if let Some(winner) = server.get_player_by_name(&winner_name) {
                    winner.show_title(TextComponent::text("VICTORY!"));
                    winner.show_subtitle(TextComponent::text("You won the Sumo match!"));
                }
                session.state = GameState::Ended;
            } else if session.alive_players.is_empty() {
                server.broadcast("Game Over! All fighters fell.");
                session.state = GameState::Ended;
            }
        }

        event
    }
}

// -----------------------------------------------------------------------------
// Plugin Lifecycle
// -----------------------------------------------------------------------------

pub struct SumoPlugin;

impl Plugin for SumoPlugin {
    fn new() -> Self {
        SumoPlugin
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "sumo".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Pumpkin Developer".into()],
            description: "A complete Sumo PvP minigame for Pumpkin Minecraft server.".into(),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let config = SumoConfig::default();
        let session = Arc::new(Mutex::new(SumoSession::new(config)));
        *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);

        let names = ["sumo".to_string(), "sm".to_string()];
        let command = Command::new(&names, "Sumo minigame management command")
            .then(CommandNode::literal("join").execute(SumoJoinHandler))
            .then(CommandNode::literal("leave").execute(SumoLeaveHandler))
            .then(CommandNode::literal("start").execute(SumoStartHandler))
            .then(CommandNode::literal("status").execute(SumoStatusHandler))
            .then(CommandNode::literal("reset").execute(SumoResetHandler))
            .execute(SumoRootHandler);

        context.register_command(command, "pumpkin.command.sumo");

        // Protect the ring platform from being broken
        context.register_event_handler(
            SumoBreakListener,
            EventPriority::High,
            true,
        )?;

        // Monitor ring-out falls
        context.register_event_handler(
            SumoMoveListener,
            EventPriority::Normal,
            false,
        )?;

        tracing::info!("Sumo minigame plugin successfully loaded!");
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<(), String> {
        tracing::info!("Sumo plugin unloaded.");
        Ok(())
    }
}

register_plugin!(SumoPlugin);
