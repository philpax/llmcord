use crate::{commands, config::Configuration, util};
use serenity::{
    all::{
        Command, Context, CreateInteractionResponse, CreateInteractionResponseMessage,
        EventHandler, Http, Interaction, MessageId, Ready,
    },
    async_trait,
};
use std::collections::HashSet;

pub struct Handler {
    handlers: Vec<Box<dyn commands::CommandHandler>>,
    cancel_tx: flume::Sender<MessageId>,
}
impl Handler {
    pub async fn new(config: Configuration) -> anyhow::Result<Self> {
        let (cancel_tx, cancel_rx) = flume::unbounded::<MessageId>();
        let handlers: Vec<Box<dyn commands::CommandHandler>> = vec![
            Box::new(
                commands::hallucinate::HallucinateHandler::new(&config, cancel_rx.clone()).await?,
            ),
            Box::new(commands::execute::Execute::new(&config, cancel_rx.clone()).await?),
        ];

        Ok(Self {
            cancel_tx,
            handlers,
        })
    }
}
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        if let Err(err) = self.ready_impl(&ctx.http, ready).await {
            println!("Error while registering commands: `{err}`");
            std::process::exit(1);
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        self.interaction_create_impl(ctx, interaction).await;
    }
}
impl Handler {
    async fn ready_impl(&self, http: &Http, ready: Ready) -> anyhow::Result<()> {
        println!("{} is connected; registering commands...", ready.user.name);

        // Check if we need to reset our registered commands
        let registered_commands: HashSet<_> = {
            let cmds = Command::get_global_commands(http).await?;
            cmds.iter().map(|c| c.name.clone()).collect()
        };
        let our_commands: HashSet<_> = self
            .handlers
            .iter()
            .flat_map(|h| h.registerable_commands())
            .collect();
        if registered_commands != our_commands {
            Command::set_global_commands(http, vec![]).await?;
        }

        for handler in &self.handlers {
            handler.register(http).await?;
        }

        println!("{} is good to go!", ready.user.name);

        Ok(())
    }

    async fn interaction_create_impl(&self, ctx: Context, interaction: Interaction) {
        let http = &ctx.http;
        match interaction {
            Interaction::Command(cmd) => {
                let name = cmd.data.name.as_str();

                if let Some(handler) = self.handlers.iter().find(|h| h.can_handle_command(&cmd)) {
                    util::run_and_report_error(&cmd, http, handler.run(http, &cmd)).await;
                } else {
                    util::run_and_report_error(&cmd, http, async {
                        anyhow::bail!("no handler found for command: {name}");
                    })
                    .await;
                }
            }
            Interaction::Component(cmp) => {
                if let ["cancel", message_id, user_id] =
                    cmp.data.custom_id.split('#').collect::<Vec<_>>()[..]
                {
                    if let (Ok(message_id), Ok(user_id)) =
                        (message_id.parse::<u64>(), user_id.parse::<u64>())
                    {
                        if cmp.user.id == user_id {
                            self.cancel_tx.send(MessageId::new(message_id)).ok();
                            cmp.create_response(
                                http,
                                CreateInteractionResponse::UpdateMessage(
                                    CreateInteractionResponseMessage::new(),
                                ),
                            )
                            .await
                            .ok();
                        }
                    }
                }
            }
            _ => {}
        };
    }
}
