//! # 02 - Custom Command Plugin Example for Pumpkin
//!
//! Demonstrates:
//! - Building and registering custom Minecraft commands using the Pumpkin Plugin API.
//! - Setting up subcommands with [`CommandNode::literal`].
//! - Defining typed arguments with [`CommandNode::argument`] and [`ArgumentType`].
//! - Implementing [`CommandHandler`] to handle command execution.
//! - Extracting consumed arguments from [`ConsumedArgs`].
//! - Sending feedback messages back to the [`CommandSender`].
//! - Accessing server methods from [`Server`].

use pumpkin_plugin_api::{
    command::{
        Arg, ArgumentType, Command, CommandError, CommandNode, CommandSender, ConsumedArgs,
        StringType,
    },
    commands::CommandHandler,
    Context, Plugin, PluginMetadata, register_plugin, Server,
    text::TextComponent,
};

/// Handler for the root `/greet` command.
struct GreetRootHandler;

impl CommandHandler for GreetRootHandler {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let sender_name = sender.get_name();
        let message = format!("Hello, {sender_name}! Welcome to Pumpkin!");
        sender.send_message(TextComponent::text(&message));
        Ok(0)
    }
}

/// Handler for `/greet broadcast <message>`.
struct GreetBroadcastHandler;

impl CommandHandler for GreetBroadcastHandler {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let message = match args.get_value("message") {
            Arg::Simple(s) | Arg::Msg(s) => s,
            _ => "Hello everyone!".to_string(),
        };

        let sender_name = sender.get_name();
        let broadcast_text = format!("[Broadcast by {sender_name}] {message}");
        server.broadcast(&broadcast_text);

        sender.send_message(TextComponent::text("Broadcast sent to all online players!"));
        Ok(0)
    }
}

/// Handler for `/greet stats`.
struct GreetStatsHandler;

impl CommandHandler for GreetStatsHandler {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let tps = server.get_tps();
        let mspt = server.get_mspt();
        let player_count = server.get_player_count();
        let max_players = server.get_max_players();

        let stats_info = format!(
            "Server Stats:\n - TPS: {:.2}\n - MSPT: {:.2}ms\n - Players: {}/{}",
            tps, mspt, player_count, max_players
        );

        sender.send_message(TextComponent::text(&stats_info));
        Ok(0)
    }
}

/// Main plugin struct for custom command example.
pub struct CustomCommandPlugin;

impl Plugin for CustomCommandPlugin {
    fn new() -> Self {
        CustomCommandPlugin
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "custom-command".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Pumpkin Developer".into()],
            description: "A custom command example for the Pumpkin Minecraft server.".into(),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let names = ["greet".to_string(), "greeting".to_string()];

        // Build command tree:
        // /greet
        // /greet broadcast <message>
        // /greet stats
        let command = Command::new(&names, "A friendly greeting and server stats command")
            .then(
                CommandNode::literal("broadcast").then(
                    CommandNode::argument("message", &ArgumentType::String(StringType::Greedy))
                        .execute(GreetBroadcastHandler),
                ),
            )
            .then(
                CommandNode::literal("stats").execute(GreetStatsHandler),
            )
            .execute(GreetRootHandler);

        context.register_command(command, "pumpkin.command.greet");
        tracing::info!("Registered /greet command with aliases and subcommands!");

        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<(), String> {
        tracing::info!("Unloaded custom command plugin!");
        Ok(())
    }
}

register_plugin!(CustomCommandPlugin);
