//! Mensagens da IDE (painel Messages). Na Fase 6 recebe tambem os erros/avisos do compilador.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Hint,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub level: Level,
    pub text: String,
}

#[derive(Default, Debug)]
pub struct MessageLog {
    entries: Vec<Message>,
}

impl MessageLog {
    /// Limite para o painel nao crescer sem fim numa sessao longa.
    pub const MAX: usize = 2000;

    pub fn push(&mut self, level: Level, text: impl Into<String>) {
        if self.entries.len() == Self::MAX {
            self.entries.remove(0);
        }
        self.entries.push(Message {
            level,
            text: text.into(),
        });
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.push(Level::Info, text);
    }

    pub fn hint(&mut self, text: impl Into<String>) {
        self.push(Level::Hint, text);
    }

    pub fn warning(&mut self, text: impl Into<String>) {
        self.push(Level::Warning, text);
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push(Level::Error, text);
    }

    pub fn entries(&self) -> &[Message] {
        &self.entries
    }

    pub fn count(&self, level: Level) -> usize {
        self.entries.iter().filter(|m| m.level == level).count()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_is_bounded() {
        let mut log = MessageLog::default();
        for i in 0..MessageLog::MAX + 5 {
            log.info(format!("m{i}"));
        }
        assert_eq!(log.entries().len(), MessageLog::MAX);
        assert_eq!(log.entries()[0].text, "m5");
    }
}
