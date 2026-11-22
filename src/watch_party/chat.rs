use serde::{Deserialize, Serialize};

use super::types::PeerId;

const MAX_CHAT: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: PeerId,
    pub name: String,
    pub text: String,
    pub ts: u64,
}

#[derive(Default)]
pub struct ChatLog {
    messages: Vec<ChatMessage>,
}

#[allow(dead_code)]
impl ChatLog {
    pub fn push(&mut self, msg: ChatMessage) {
        let text: String = msg.text.trim().chars().take(500).collect();
        if text.is_empty() {
            return;
        }
        let mut msg = msg;
        msg.text = text;
        self.messages.push(msg);
        if self.messages.len() > MAX_CHAT {
            let drop = self.messages.len() - MAX_CHAT;
            self.messages.drain(0..drop);
        }
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_long_messages_are_trimmed() {
        let mut log = ChatLog::default();
        log.push(ChatMessage {
            id: 1,
            name: "a".into(),
            text: "   ".into(),
            ts: 1,
        });
        assert!(log.is_empty());
        log.push(ChatMessage {
            id: 1,
            name: "a".into(),
            text: "x".repeat(600),
            ts: 1,
        });
        assert_eq!(log.messages()[0].text.chars().count(), 500);
    }

    #[test]
    fn log_is_bounded() {
        let mut log = ChatLog::default();
        for i in 0..250 {
            log.push(ChatMessage {
                id: i,
                name: "a".into(),
                text: "x".into(),
                ts: i,
            });
        }
        assert_eq!(log.len(), 200);
        assert_eq!(log.messages()[0].id, 50);
    }
}
