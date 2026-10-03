use crate::types::{Error, ToolCall, ToolDef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Text(String),
    Call(ToolCall),
}

pub struct StreamDecoder {
    tools: Vec<ToolDef>,
    buffer: String,
}

impl StreamDecoder {
    pub fn new(tools: &[ToolDef]) -> Self {
        Self {
            tools: tools.to_vec(),
            buffer: String::new(),
        }
    }

    pub fn push(&mut self, chunk: &str) -> Result<Vec<Event>, Error> {
        self.buffer.push_str(chunk);
        let mut events = Vec::new();

        loop {
            let Some(start) = first_call_start(&self.buffer) else {
                if is_partial_call_start(&self.buffer) {
                    break;
                }
                let text = std::mem::take(&mut self.buffer);
                if !text.is_empty() {
                    Self::push_event(&mut events, Event::Text(text));
                }
                break;
            };

            let candidate = &self.buffer[start..];
            match crate::decode::parse_call(candidate, &self.tools) {
                Ok((call, consumed)) => {
                    let before = &self.buffer[..start];
                    if !before.is_empty() {
                        Self::push_event(&mut events, Event::Text(before.to_string()));
                    }
                    self.buffer = candidate[consumed..].to_string();
                    Self::push_event(&mut events, Event::Call(call));
                }
                Err(_) if is_partial_call_start(candidate) || !candidate.contains(">>") => {
                    break;
                }
                Err(err) => return Err(err),
            }
        }

        Ok(events)
    }

    pub fn finish(&mut self) -> Result<Vec<Event>, Error> {
        if self.buffer.is_empty() {
            return Ok(Vec::new());
        }
        if self.buffer.contains("<<call") {
            let start = first_call_start(&self.buffer).unwrap_or(0);
            let candidate = &self.buffer[start..];
            if candidate.starts_with("<<call") {
                return Err(Error::MalformedCall);
            }
        }
        let text = std::mem::take(&mut self.buffer);
        let mut events = Vec::new();
        Self::push_event(&mut events, Event::Text(text));
        Ok(events)
    }

    fn push_event(events: &mut Vec<Event>, event: Event) {
        match event {
            Event::Text(s) if s.is_empty() => {}
            Event::Text(s) => {
                if let Some(Event::Text(last)) = events.last_mut() {
                    last.push_str(&s);
                } else {
                    events.push(Event::Text(s));
                }
            }
            call => events.push(call),
        }
    }
}

fn first_call_start(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    for i in 0..(s.len().saturating_sub(2)) {
        if bytes[i] == b'<' && bytes[i + 1] == b'<' {
            let tail = &s[i + 2..];
            if tail.starts_with("call") || tail.starts_with("cal") || tail.starts_with("ca") || tail.starts_with("c") {
                return Some(i);
            }
        }
    }
    None
}

fn is_partial_call_start(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let starts = ["c", "ca", "cal", "call"];
    for prefix in starts {
        if s.ends_with(prefix) {
            return true;
        }
    }
    s.ends_with("<") || s.ends_with("<<")
}
