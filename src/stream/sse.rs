use std::pin::Pin;

use futures::stream::{Stream, StreamExt};
use reqwest_eventsource::{Event, EventSource};

use crate::Result;

pub struct SseStream {
    event_source: EventSource,
}

impl SseStream {
    pub fn new(event_source: EventSource) -> Self {
        Self { event_source }
    }

    pub fn into_stream(self) -> Pin<Box<dyn Stream<Item = Result<String>> + Send>> {
        let mut es = self.event_source;

        let stream = async_stream::stream! {
            loop {
                match es.next().await {
                    Some(Ok(Event::Open)) => continue,
                    Some(Ok(Event::Message(msg))) => {
                        let data = msg.data.trim().to_string();
                        if data.is_empty() || data.starts_with(':') {
                            continue;
                        }
                        if data == "[DONE]" {
                            break;
                        }
                        yield Ok(data);
                    }
                    Some(Err(e)) => {
                        yield Err(crate::AiError::Stream(e.to_string()));
                        break;
                    }
                    None => break,
                }
            }
            es.close();
        };

        Box::pin(stream)
    }
}
