// Extensions to MassiveGameServer for async bot AI integration
use crate::systems::ai::async_bot_ai::{AsyncBotAI, BotDecision};
use std::sync::Arc;
use tokio::task::JoinHandle;

impl MassiveGameServer {
    /// Initialize and start the async bot AI system
    pub fn start_async_bot_ai(&self) -> (AsyncBotAI, JoinHandle<()>) {
        let mut bot_ai = AsyncBotAI::new();
        let server_arc = Arc::new(self.clone()); // Assuming MassiveGameServer is cloneable
        
        // Start the async bot AI task
        let ai_handle = AsyncBotAI::start_bot_ai_task(
            server_arc,
            bot_ai.decision_sender.clone()
        );
        
        info!("Async bot AI system started");
        (bot_ai, ai_handle)
    }
    
    /// Poll and apply bot decisions - call this in the game loop
    pub fn process_async_bot_decisions(&self, bot_ai: &mut AsyncBotAI) {
        bot_ai.poll_and_apply_decisions(self);
    }
}
