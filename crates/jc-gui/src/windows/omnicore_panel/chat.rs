use super::Answer;
use std::collections::VecDeque;

const MAX_TURNS: usize = 8;
const CURRENT: &str = "Solicitud actual:\n";
const CONTEXT: &str = "Contexto de código adjuntado (datos):\n";

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Ask,
    Plan,
    Edit,
}
impl Mode {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Ask => "Consulta",
            Self::Plan => "Plan",
            Self::Edit => "Editar",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Attachment {
    pub title: String,
    pub text: String,
}

pub(super) struct Turn {
    pub mode: Mode,
    pub question: String,
    pub model: Option<String>,
    pub attachment: Option<String>,
    pub answer: Option<Result<Answer, String>>,
}

#[derive(Default)]
pub(super) struct Chat {
    pub mode: Mode,
    pub attachment: Option<Attachment>,
    pub turns: VecDeque<Turn>,
}

#[derive(PartialEq, Eq)]
pub(super) struct Outgoing {
    pub input: String,
    pub attachment_title: Option<String>,
}

pub(super) struct Prompt<'a> {
    prefix: String,
    question: &'a str,
    remaining: usize,
}

impl Prompt<'_> {
    pub fn context_bytes(&self, title: &str) -> usize {
        self.remaining
            .saturating_sub(context_section(title, "").len())
    }

    pub fn finish(mut self, attachment: Option<&Attachment>) -> Result<Outgoing, String> {
        if let Some(attachment) = attachment {
            let section = context_section(&attachment.title, &attachment.text);
            if section.len() > self.remaining {
                return Err("El adjunto supera el espacio disponible del modelo. Adjunta una selección más corta.".into());
            }
            self.prefix.push_str(&section);
        }
        self.prefix.push_str(CURRENT);
        self.prefix.push_str(self.question);
        Ok(Outgoing {
            input: self.prefix,
            attachment_title: attachment.map(|attachment| attachment.title.clone()),
        })
    }
}

fn context_section(title: &str, text: &str) -> String {
    let data = serde_json::json!({ "archivo": title, "texto": text });
    format!("{CONTEXT}{data}\n\n")
}

pub(super) fn escaped_bytes(character: char) -> usize {
    match character {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        '\0'..='\u{1f}' => 6,
        other => other.len_utf8(),
    }
}

impl Chat {
    pub fn prompt<'a>(
        &self,
        question: &'a str,
        capacity: usize,
        reserve_context: bool,
    ) -> Result<Prompt<'a>, String> {
        let mut prefix = match self.mode {
            Mode::Ask | Mode::Edit => String::new(),
            Mode::Plan => String::from(
                "Modo Plan. Responde en español con un plan concreto para la solicitud. Incluye archivos, pasos y cómo validar.\n\n",
            ),
        };
        let remaining = capacity
            .checked_sub(prefix.len() + CURRENT.len() + question.len())
            .ok_or("La consulta supera el espacio de entrada del modelo. Acorta la pregunta o elige otro modelo.")?;
        let history_budget = if let Some(attachment) = self.attachment.as_ref() {
            remaining.saturating_sub(context_section(&attachment.title, &attachment.text).len())
        } else if reserve_context {
            remaining / 4
        } else {
            remaining
        };
        let mut history = Vec::new();
        let mut used = 0;
        for turn in self.turns.iter().rev() {
            let Some(Ok(answer)) = &turn.answer else {
                continue;
            };
            if !answer.complete {
                continue;
            }
            let prefix = format!("Usuario: {}\nOmniCore: ", turn.question);
            let available = history_budget.saturating_sub(used + prefix.len() + 2);
            if available < 32 {
                break;
            }
            let mut end = available.min(answer.text.len());
            while !answer.text.is_char_boundary(end) {
                end -= 1;
            }
            history.push(format!("{prefix}{}\n\n", &answer.text[..end]));
            used += prefix.len() + end + 2;
        }
        for turn in history.into_iter().rev() {
            prefix.push_str(&turn);
        }
        Ok(Prompt {
            prefix,
            question,
            remaining: remaining - used,
        })
    }

    pub fn sent(&mut self, question: String, automatic_title: Option<String>) {
        if self.turns.len() == MAX_TURNS {
            self.turns.pop_front();
        }
        self.turns.push_back(Turn {
            mode: self.mode,
            question,
            model: None,
            attachment: self
                .attachment
                .take()
                .map(|attachment| attachment.title)
                .or(automatic_title),
            answer: None,
        });
    }

    pub fn finish(&mut self, answer: Result<Answer, String>) {
        if let Some(turn) = self.turns.back_mut() {
            turn.answer = Some(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omnicore_prompt_reuses_followups_and_preserves_explicit_context() {
        let mut chat = Chat::default();
        assert_eq!(
            chat.prompt("Primera pregunta", 1024, false)
                .unwrap()
                .finish(None)
                .unwrap()
                .input,
            "Solicitud actual:\nPrimera pregunta"
        );
        chat.sent("Primera pregunta".into(), None);
        chat.finish(Ok(Answer {
            activity: Vec::new(),
            text: "Una explicación anterior.".into(),
            complete: true,
        }));
        chat.sent("Falló".into(), None);
        chat.finish(Err("Error".into()));
        chat.mode = Mode::Plan;
        chat.attachment = Some(Attachment {
            title: "programa.p".into(),
            text: "DISPLAY \"ñ\".\nQUIT.".into(),
        });
        let prompt = chat
            .prompt("Siguiente pregunta", 1024, false)
            .unwrap()
            .finish(chat.attachment.as_ref())
            .unwrap()
            .input;
        assert!(prompt.starts_with("Modo Plan."));
        assert!(prompt.contains("Una explicación anterior."));
        assert!(prompt.contains(r#"DISPLAY \"ñ\".\nQUIT."#));
        assert!(!prompt.contains("Falló"));
        assert!(prompt.ends_with("Siguiente pregunta"));
        chat.sent("Siguiente pregunta".into(), None);
        assert!(chat.attachment.is_none());
        assert_eq!(
            chat.turns.back().unwrap().attachment.as_deref(),
            Some("programa.p")
        );
        assert!(chat.prompt(&"ñ".repeat(1024), 1024, false).is_err());
    }
}
