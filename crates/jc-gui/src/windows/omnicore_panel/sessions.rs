use super::chat::Chat;
use jc_app::FieldSelection;
use std::collections::VecDeque;

const MAX_SESSIONS: usize = 8;

#[derive(Default)]
pub(super) struct Conversation {
    pub id: u64,
    pub chat: Chat,
    pub draft: String,
    pub selection: FieldSelection,
    pub first: Option<usize>,
    pub omit_active_file: bool,
}
impl Conversation {
    pub fn title(&self) -> &str {
        self.chat
            .turns
            .front()
            .map(|turn| turn.question.as_str())
            .unwrap_or(self.draft.as_str())
            .lines()
            .find(|line| !line.trim().is_empty())
            .map_or("Conversación nueva", str::trim)
    }
}

#[derive(Default)]
pub(super) struct Sessions {
    pub active: Conversation,
    recent: VecDeque<Conversation>,
    latest_id: u64,
}
impl Sessions {
    pub fn iter(&self) -> impl Iterator<Item = &Conversation> {
        std::iter::once(&self.active).chain(&self.recent)
    }

    pub fn len(&self) -> usize {
        1 + self.recent.len()
    }

    pub fn new_conversation(&mut self, protected: Option<u64>) {
        self.latest_id += 1;
        let next = Conversation {
            id: self.latest_id,
            ..Default::default()
        };
        self.activate(next);
        if self.recent.len() >= MAX_SESSIONS {
            let last = self
                .recent
                .iter()
                .rposition(|item| Some(item.id) != protected)
                .unwrap();
            self.recent.remove(last);
        }
    }

    pub fn choose(&mut self, id: u64) {
        if let Some(index) = self.recent.iter().position(|item| item.id == id) {
            let previous = self.recent.remove(index).unwrap();
            self.activate(previous);
        }
    }

    pub fn conversation_mut(&mut self, id: u64) -> Option<&mut Conversation> {
        if self.active.id == id {
            Some(&mut self.active)
        } else {
            self.recent.iter_mut().find(|item| item.id == id)
        }
    }

    fn activate(&mut self, next: Conversation) {
        self.recent
            .push_front(std::mem::replace(&mut self.active, next));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omnicore_sessions_keep_the_query_owner_when_retention_is_full() {
        let mut sessions = Sessions::default();
        let origin = sessions.active.id;
        sessions.active.chat.sent("Revisar proyecto".into(), None);
        sessions.active.draft = "seguimiento".into();
        for _ in 0..MAX_SESSIONS * 2 {
            sessions.new_conversation(Some(origin));
        }
        assert_eq!(sessions.len(), MAX_SESSIONS);
        sessions
            .conversation_mut(origin)
            .unwrap()
            .chat
            .finish(Err("cancelada".into()));
        sessions.choose(origin);
        assert_eq!(sessions.active.draft, "seguimiento");
        assert!(matches!(
            sessions.active.chat.turns[0].answer.as_ref(),
            Some(Err(message)) if message == "cancelada"
        ));
    }
}
