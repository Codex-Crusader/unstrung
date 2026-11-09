//! Cleaning and checking the text that the user types.

use super::*;

/// `/ooc text` or `((text))` is an out-of-character note.
pub(super) fn split_ooc(text: &str) -> Result<(&'static str, String)> {
    let note = match text.get(..4) {
        Some(head)
            if head.eq_ignore_ascii_case("/ooc")
                && (text.len() == 4 || text[4..].starts_with(char::is_whitespace)) =>
        {
            Some(text[4..].trim().to_owned())
        }
        _ if text.starts_with("((") && text.ends_with("))") && text.len() >= 4 => {
            Some(text[2..text.len() - 2].trim().to_owned())
        }
        _ => None,
    };
    match note {
        Some(n) if n.is_empty() => Err(Error::Invalid("the note is empty".into())),
        Some(n) => Ok(("ooc", n)),
        None => Ok(("user", text.to_owned())),
    }
}

/// Trims the text, removes control characters, and checks the length.
pub fn clean_input(text: &str) -> Result<String> {
    clean_text(text, MAX_MESSAGE_CHARS)
}

/// `clean_input` with another length limit, for example for imported replies.
pub fn clean_text(text: &str, max: usize) -> Result<String> {
    let text: String = text
        .trim()
        .chars()
        .filter(|c| (!c.is_control() || matches!(c, '\n' | '\t')) && !is_bidi_control(*c))
        .collect();
    if text.is_empty() {
        return Err(Error::Invalid("the message is empty".into()));
    }
    if text.chars().count() > max {
        return Err(Error::Invalid(format!(
            "the message is longer than {max} characters"
        )));
    }
    // The hard limit holds for what the user writes too, so a story never
    // stores such text, and the model never reads it.
    if crate::pictures::safety::reply_crosses_line(&text) {
        log::warn!("a message was refused: explicit words next to a minor");
        return Err(Error::Rejected {
            message: "Unstrung does not take sexual content that involves anyone under 18. Change the message.".into(),
            why: "explicit words next to a minor".into(),
        });
    }
    Ok(text)
}

/// One line of plain text: no control or direction characters, cut to `max` characters.
pub fn clean_line(text: &str, max: usize) -> String {
    text.chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control() && !is_bidi_control(*c))
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Right-to-left overrides and isolates can make text look like something else.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')
}
