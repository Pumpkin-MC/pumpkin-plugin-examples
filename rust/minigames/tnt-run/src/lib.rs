//! # TNT Run Minigame for Pumpkin
//!
//! A complete, runnable TNT Run minigame showcasing:
//! - State management (`Waiting`, `Active`, `Ended`).
//! - Commands: `/tntrun join`, `/tntrun leave`, `/tntrun start`, `/tntrun status`, `/tntrun reset`.
//! - Movement tracking with [`PlayerMoveEvent`]:
//!   - Decaying blocks under players' feet with a delay.
//!   - Detecting when a player falls below the arena elimination Y level.
//! - Automatic win condition check (last player standing).
//! - Clean floor regeneration when resetting.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use pumpkin_plugin_api::{
    command::{Command, CommandError, CommandNode, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    common::{BlockPos, Position},
    events::{EventHandler, EventPriority, player::PlayerMoveEvent},
    events_wit::PlayerMoveEventData,
    scheduler::SchedulerExt,
    world::BlockFlags,
    Context, Plugin, PluginMetadata, register_plugin, Server,
    text::TextComponent,
};

/// Arena spatial configuration.
#[derive(Clone, Debug)]
pub struct ArenaConfig {
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
    /// Y coordinates of the running floors (top to bottom)
    pub floor_y_levels: Vec<i32>,
    /// Any player falling below this Y level is eliminated
    pub elimination_y: f64,
    /// Spawn position where players start the game
    pub spawn_pos: Position,
    /// Lobby position where spectators / eliminated players wait
    pub lobby_pos: Position,
}

impl Default for ArenaConfig {
    fn default() -> Self {
        Self {
            min_x: -15,
            max_x: 15,
            min_z: -15,
            max_z: 15,
            floor_y_levels: vec![100, 85, 70],
            elimination_y: 60.0,
            spawn_pos: (0.5, 101.0, 0.5),
            lobby_pos: (0.5, 115.0, 0.5),
        }
    }
}

impl ArenaConfig {
    pub fn is_inside_xz(&self, x: f64, z: f64) -> bool {
        let ix = x.floor() as i32;
        let iz = z.floor() as i32;
        ix >= self.min_x && ix <= self.max_x && iz >= self.min_z && iz <= self.max_z
    }
}

/// Lifecycle state of the TNT Run game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameState {
    Waiting,
    Active,
    Ended,
}

/// Shared game session state.
pub struct GameSession {
    pub state: GameState,
    pub config: ArenaConfig,
    pub queued_players: HashSet<String>,
    pub alive_players: HashSet<String>,
    pub recently_decaying: HashSet<(i32, i32, i32)>,
}

impl GameSession {
    pub fn new(config: ArenaConfig) -> Self {
        Self {
            state: GameState::Waiting,
            config,
            queued_players: HashSet::new(),
            alive_players: HashSet::new(),
            recently_decaying: HashSet::new(),
        }
    }
}

static SESSION: Mutex<Option<Arc<Mutex<GameSession>>>> = Mutex::new(None);

fn get_session() -> Arc<Mutex<GameSession>> {
    SESSION
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .expect("GameSession must be initialized in on_load")
}

// -----------------------------------------------------------------------------
// Commands: /tntrun <subcommand>
// -----------------------------------------------------------------------------

struct TntRunRootHandler;
impl CommandHandler for TntRunRootHandler {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let help_text = "TNT Run Commands:\n\
            /tntrun join   - Join the game queue\n\
            /tntrun leave  - Leave the queue or game\n\
            /tntrun start  - Force start the game\n\
            /tntrun status - View game status\n\
            /tntrun reset  - Regenerate arena floors";
        sender.send_message(TextComponent::text(help_text));
        Ok(0)
    }
}

struct TntRunJoinHandler;
impl CommandHandler for TntRunJoinHandler {
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
                    server.broadcast(&format!("{player_name} joined TNT Run! ({count} players queued)"));
                    sender.send_message(TextComponent::text("You joined the queue! Use /tntrun start when ready."));
                } else {
                    sender.send_message(TextComponent::text("You are already in the queue!"));
                }
            }
        }
        Ok(0)
    }
}

struct TntRunLeaveHandler;
impl CommandHandler for TntRunLeaveHandler {
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
            server.broadcast(&format!("{player_name} left TNT Run."));
            sender.send_message(TextComponent::text("You left TNT Run."));
        } else {
            sender.send_message(TextComponent::text("You are not currently in a TNT Run game."));
        }
        Ok(0)
    }
}

struct TntRunStartHandler;
impl CommandHandler for TntRunStartHandler {
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

        // If no one is queued, queue the sender if it's a player
        if session.queued_players.is_empty() {
            session.queued_players.insert(sender.get_name());
        }

        session.alive_players = session.queued_players.clone();
        session.state = GameState::Active;
        session.recently_decaying.clear();

        let count = session.alive_players.len();
        server.broadcast(&format!("TNT Run has STARTED with {count} players! Keep running!"));

        // Teleport all alive players to the spawn position
        let spawn = session.config.spawn_pos;
        for player_name in &session.alive_players {
            if let Some(player) = server.get_player_by_name(player_name) {
                let world = player.get_world();
                player.teleport(spawn, Some(0.0), Some(0.0), world);
                player.show_title(TextComponent::text("TNT RUN!"));
                player.show_subtitle(TextComponent::text("Don't stop moving!"));
            }
        }

        Ok(0)
    }
}

struct TntRunStatusHandler;
impl CommandHandler for TntRunStatusHandler {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let session_arc = get_session();
        let session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        let status_msg = match session.state {
            GameState::Waiting => format!("TNT Run: WAITING ({} players queued)", session.queued_players.len()),
            GameState::Active => format!(
                "TNT Run: ACTIVE ({} alive: {:?})",
                session.alive_players.len(),
                session.alive_players
            ),
            GameState::Ended => "TNT Run: ENDED (Use /tntrun reset or /tntrun start)".to_string(),
        };

        sender.send_message(TextComponent::text(&status_msg));
        Ok(0)
    }
}

struct TntRunResetHandler;
impl CommandHandler for TntRunResetHandler {
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
        session.recently_decaying.clear();

        // Regenerate floors using the first available world
        let worlds = server.get_all_worlds();
        if let Some(world) = worlds.first() {
            let flags = BlockFlags::NOTIFY_LISTENERS | BlockFlags::NOTIFY_NEIGHBORS;
            for &y in &session.config.floor_y_levels {
                for x in session.config.min_x..=session.config.max_x {
                    for z in session.config.min_z..=session.config.max_z {
                        let pos = BlockPos { x, y, z };
                        world.set_block_by_name(pos, "minecraft:sand", flags);
                        let tnt_pos = BlockPos { x, y: y - 1, z };
                        world.set_block_by_name(tnt_pos, "minecraft:tnt", flags);
                    }
                }
            }
        }

        server.broadcast("TNT Run arena has been regenerated and reset!");
        sender.send_message(TextComponent::text("Arena reset complete!"));
        Ok(0)
    }
}

// -----------------------------------------------------------------------------
// Movement & Decay Event Listener
// -----------------------------------------------------------------------------

struct TntRunMoveListener;

impl EventHandler<PlayerMoveEvent> for TntRunMoveListener {
    fn handle(&self, server: Server, event: PlayerMoveEventData) -> PlayerMoveEventData {
        let player_name = event.player.get_name();
        let session_arc = get_session();
        let mut session = session_arc.lock().unwrap_or_else(|e| e.into_inner());

        // Only process movement if game is active and player is an alive participant
        if session.state != GameState::Active || !session.alive_players.contains(&player_name) {
            return event;
        }

        let to = &event.to_position;

        // 1. Elimination Check: player fell below the elimination threshold
        if to.1 < session.config.elimination_y {
            session.alive_players.remove(&player_name);
            server.broadcast(&format!("{player_name} fell into the void and was ELIMINATED!"));

            // Teleport eliminated player back to lobby
            let lobby = session.config.lobby_pos;
            let world = event.player.get_world();
            event.player.teleport(lobby, Some(0.0), Some(0.0), world);
            event.player.show_title(TextComponent::text("ELIMINATED!"));

            // 2. Win Condition Check
            if session.alive_players.len() == 1 {
                let winner_name = session.alive_players.iter().next().cloned().unwrap();
                server.broadcast(&format!("VICTORY! {winner_name} is the last player standing and WINS TNT Run!"));
                if let Some(winner) = server.get_player_by_name(&winner_name) {
                    winner.show_title(TextComponent::text("VICTORY!"));
                    winner.show_subtitle(TextComponent::text("You won TNT Run!"));
                }
                session.state = GameState::Ended;
            } else if session.alive_players.is_empty() {
                server.broadcast("Game Over! No players remaining.");
                session.state = GameState::Ended;
            }

            return event;
        }

        // 3. Block Decay Mechanic: if inside arena XZ, decay the block under feet
        if session.config.is_inside_xz(to.0, to.2) {
            let bx = to.0.floor() as i32;
            let by = (to.1 - 0.2).floor() as i32;
            let bz = to.2.floor() as i32;

            // Only trigger decay if standing on one of the designated floor Y levels
            if session.config.floor_y_levels.contains(&by) && session.recently_decaying.insert((bx, by, bz)) {
                let world = event.player.get_world();
                let flags = BlockFlags::NOTIFY_LISTENERS | BlockFlags::NOTIFY_NEIGHBORS;

                // Turn sand and underlying TNT to air after delay
                let session_clone = Arc::clone(&session_arc);
                server.schedule_delayed_task(8, move |_| {
                    let pos = BlockPos { x: bx, y: by, z: bz };
                    let tnt_pos = BlockPos { x: bx, y: by - 1, z: bz };
                    world.set_block_by_name(pos, "minecraft:air", flags);
                    world.set_block_by_name(tnt_pos, "minecraft:air", flags);

                    if let Ok(mut s) = session_clone.lock() {
                        s.recently_decaying.remove(&(bx, by, bz));
                    }
                });
            }
        }

        event
    }
}

// -----------------------------------------------------------------------------
// Plugin Lifecycle
// -----------------------------------------------------------------------------

pub struct TntRunPlugin;

impl Plugin for TntRunPlugin {
    fn new() -> Self {
        TntRunPlugin
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "tnt-run".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Pumpkin Developer".into()],
            description: "A complete TNT Run minigame for Pumpkin Minecraft server.".into(),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let config = ArenaConfig::default();
        let session = Arc::new(Mutex::new(GameSession::new(config)));
        *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);

        // Register /tntrun command tree
        let names = ["tntrun".to_string(), "tr".to_string()];
        let command = Command::new(&names, "TNT Run minigame management command")
            .then(CommandNode::literal("join").execute(TntRunJoinHandler))
            .then(CommandNode::literal("leave").execute(TntRunLeaveHandler))
            .then(CommandNode::literal("start").execute(TntRunStartHandler))
            .then(CommandNode::literal("status").execute(TntRunStatusHandler))
            .then(CommandNode::literal("reset").execute(TntRunResetHandler))
            .execute(TntRunRootHandler);

        context.register_command(command, "pumpkin.command.tntrun");

        // Register PlayerMoveEvent listener
        context.register_event_handler(
            TntRunMoveListener,
            EventPriority::Normal,
            false,
        )?;

        tracing::info!("TNT Run minigame plugin successfully loaded!");
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<(), String> {
        tracing::info!("TNT Run plugin unloaded.");
        Ok(())
    }
}

register_plugin!(TntRunPlugin);
