use crate::core::constants::*;
use crate::flatbuffers_generated::game_protocol as fb;
use bytes::Bytes;
use std::{
    cell::RefCell,
    time::{SystemTime, UNIX_EPOCH},
};

thread_local! {
    static WELCOME_FB_BUILDER: RefCell<flatbuffers::FlatBufferBuilder<'static>> =
        RefCell::new(flatbuffers::FlatBufferBuilder::with_capacity(256));
}

pub(super) fn is_bidi_or_directional_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'
            | '\u{202B}'
            | '\u{202C}'
            | '\u{202D}'
            | '\u{202E}'
            | '\u{2066}'
            | '\u{2067}'
            | '\u{2068}'
            | '\u{2069}'
    )
}

pub(super) fn sanitize_text_field(
    raw: &str,
    max_chars: usize,
    username_mode: bool,
) -> Option<String> {
    if max_chars == 0 {
        return None;
    }

    let mut cleaned = String::with_capacity(raw.len().min(max_chars));
    let mut count = 0usize;
    let mut last_was_space = true;

    for ch in raw.chars() {
        if (ch.is_control() && !ch.is_whitespace()) || is_bidi_or_directional_control(ch) {
            continue;
        }
        let normalized = if ch.is_whitespace() { ' ' } else { ch };
        if matches!(
            normalized,
            '<' | '>' | '`' | '&' | '"' | '\'' | '\\' | '/' | '{' | '}'
        ) {
            continue;
        }

        if username_mode
            && !(normalized.is_alphanumeric()
                || normalized == '_'
                || normalized == '-'
                || normalized == '.'
                || normalized == ' ')
        {
            continue;
        }

        if normalized == ' ' {
            if last_was_space {
                continue;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
        }

        cleaned.push(normalized);
        count += 1;
        if count >= max_chars {
            break;
        }
    }

    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

pub(super) fn sanitize_chat_field(raw: &str, max_chars: usize) -> Option<String> {
    sanitize_text_field(raw, max_chars, false)
}

pub(super) fn sanitize_username_field(raw: &str, max_chars: usize) -> Option<String> {
    sanitize_text_field(raw, max_chars, true)
}

pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn signaling_protocol_version() -> u32 {
    GAME_PROTOCOL_VERSION
}

pub(super) fn build_welcome_message_bytes(player_id: &str, server_tick_rate: u16) -> Bytes {
    WELCOME_FB_BUILDER.with(|builder_cell| {
        let mut builder_welcome = builder_cell.borrow_mut();
        builder_welcome.reset();
        let player_id_fb_welcome = builder_welcome.create_string(player_id);
        let welcome_text_fb = builder_welcome.create_string("Welcome to MassiveGameServer!");
        let welcome_msg_args = fb::WelcomeMessageArgs {
            player_id: Some(player_id_fb_welcome),
            message: Some(welcome_text_fb),
            server_tick_rate,
            server_protocol_version: signaling_protocol_version(),
            schema_version: 6,
        };
        let welcome_msg = fb::WelcomeMessage::create(&mut builder_welcome, &welcome_msg_args);
        let game_msg_welcome_args = fb::GameMessageArgs {
            msg_type: fb::MessageType::Welcome,
            actual_message_type: fb::MessagePayload::WelcomeMessage,
            actual_message: Some(welcome_msg.as_union_value()),
            protocol_version: GAME_PROTOCOL_VERSION,
        };
        let game_msg_welcome =
            fb::GameMessage::create(&mut builder_welcome, &game_msg_welcome_args);
        builder_welcome.finish(game_msg_welcome, None);
        Bytes::copy_from_slice(builder_welcome.finished_data())
    })
}

pub(super) fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
pub(super) fn env_bool(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}
