//! The growth judge. It scores saved replies one at a time, from the card's
//! personality, the user's line and the reply only, so it never knows which
//! build wrote a reply. Run it after the chats, so the chat model and the
//! judge never swap in the GPU between turns.

use crate::{Fail, Reply};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use unstrung_core::catalog::Tuning;
use unstrung_core::ollama::{ChatMessage, ChatParams, Client, Options};

const SYSTEM: &str = "You rate one reply from a roleplay chat. Answer with JSON only.";
const TIMEOUT: Duration = Duration::from_secs(120);

/// The saved replies of one run, for the judge and the scoring script.
pub fn dump(
    model: &str,
    script: &str,
    label: &str,
    cards: serde_json::Map<String, Value>,
    replies: &[Reply],
) -> Value {
    let replies: Vec<Value> = replies
        .iter()
        .map(|r| {
            json!({
                "character": r.character,
                "style": r.style,
                "turn": r.turn,
                "said": r.said,
                "mark": r.mark,
                "text": r.text,
                "seconds": r.seconds,
                "prompt_tokens": r.prompt_tokens,
                "prompt_read_ms": r.prompt_read_ms,
                "repetition": r.flags.repetition,
                "break_final": r.flags.break_final,
                "speaks_for_user": r.flags.speaks_for_user,
                "restarts": r.restarts,
                "critic_ms": r.critic.map(|c| c.millis),
                "critic_prompt_tokens": r.critic.and_then(|c| c.prompt_tokens),
                "critic_read_ms": r.critic.and_then(|c| c.prompt_read_ms),
                "critic_answer_tokens": r.critic.and_then(|c| c.answer_tokens),
                "critic_continued": r.critic.map(|c| c.continued),
                "critic_parsed": r.critic.map(|c| c.parsed),
                "drives": r.drives.map(|d| [d.trust, d.closeness, d.respect, d.romance]),
                "appraisal": r.appraisal,
            })
        })
        .collect();
    json!({ "model": model, "script": script, "label": label, "cards": cards, "replies": replies })
}

/// Scores of one reply: how the character feels about the user (1 cold to
/// 5 warm), and whether the reply breaks character. A mood change never
/// counts as a break: small judges scored a hurt but true reply as a
/// different person, so a 1 to 5 fit score would punish growth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub stance: u8,
    pub breaks: bool,
}

async fn score(
    client: &Client,
    model: &str,
    tuning: &Tuning,
    personality: &str,
    said: &str,
    text: &str,
) -> Result<Score, Fail> {
    let question = format!(
        "Character description: {personality}\n\n\
         The user wrote: {said}\n\n\
         The character replied: {text}\n\n\
         Rate only the character's reply.\n\
         stance: how the character feels about the user in this reply. \
         1 = hostile or cold, 2 = guarded or annoyed, 3 = neutral, 4 = friendly, 5 = warm and close. \
         Rate the feeling, not the manner: a blunt or gruff person can be warm in few words.\n\
         breaks: true when the reply is not this character speaking in the story: \
         it talks as an AI or an assistant, talks about the story or the chat itself, \
         or the speaker is plainly someone else, with another name, job or way of life. \
         Feelings about the user never count: people turn cold when they are hurt and warm \
         when they are treated well, and stay in character."
    );
    let messages = [
        ChatMessage {
            role: "system",
            content: SYSTEM.into(),
            images: Vec::new(),
        },
        ChatMessage {
            role: "user",
            content: question,
            images: Vec::new(),
        },
    ];
    let options = Options {
        temperature: 0.0,
        min_p: 0.0,
        top_p: 1.0,
        top_k: 1,
        presence_penalty: 0.0,
        repeat_penalty: 1.0,
        repeat_last_n: 64,
        // The chat's own size: another size makes the engine load the model again.
        num_ctx: tuning.context,
        num_predict: 40,
        stop: Vec::new(),
    };
    let one_to_five = json!({ "type": "integer", "minimum": 1, "maximum": 5 });
    let schema = json!({
        "type": "object",
        "properties": { "stance": one_to_five, "breaks": { "type": "boolean" } },
        "required": ["stance", "breaks"],
    });
    let params = ChatParams {
        model,
        messages: &messages,
        options: &options,
        keep_alive: unstrung_core::engine::KEEP_ALIVE,
        think: tuning.think.map(|_| false),
        format: Some(&schema),
    };
    let answer: Value = serde_json::from_str(&client.complete(&params, TIMEOUT).await?)?;
    let stance = answer["stance"]
        .as_u64()
        .filter(|n| (1..=5).contains(n))
        .ok_or_else(|| format!("the judge gave no stance from 1 to 5: {answer}"))?;
    let breaks = answer["breaks"]
        .as_bool()
        .ok_or_else(|| format!("the judge gave no breaks: {answer}"))?;
    Ok(Score {
        stance: stance as u8,
        breaks,
    })
}

/// Scores every reply in a saved run and writes FILE-judged.json next to it.
pub async fn files(client: &Client, model: &str, tuning: &Tuning, file: &Path) -> Result<(), Fail> {
    let mut run: Value = serde_json::from_str(&std::fs::read_to_string(file)?)?;
    let cards = run["cards"].clone();
    let replies = run["replies"]
        .as_array_mut()
        .ok_or("the file has no replies")?;
    let total = replies.len();
    for (i, r) in replies.iter_mut().enumerate() {
        let name = r["character"].as_str().unwrap_or_default();
        let personality = cards[name]
            .as_str()
            .ok_or_else(|| format!("the file has no card for {name}"))?;
        let s = score(
            client,
            model,
            tuning,
            personality,
            r["said"].as_str().unwrap_or_default(),
            r["text"].as_str().unwrap_or_default(),
        )
        .await?;
        r["stance"] = s.stance.into();
        r["breaks"] = s.breaks.into();
        if (i + 1) % 50 == 0 {
            eprintln!("judged {} of {total}", i + 1);
        }
    }
    run["judge"] = model.into();
    let out = file.with_extension("judged.json");
    std::fs::write(&out, serde_json::to_string_pretty(&run)?)?;
    eprintln!("judged: {}", out.display());
    Ok(())
}

/// Checks the judge on hand-written replies with known answers, twice. Each
/// fixture has `personality`, `said`, `text`, and `stance` as "high" (4 or
/// 5) or "low" (1 or 2) and or `breaks` as true or false.
pub async fn fixtures(
    client: &Client,
    model: &str,
    tuning: &Tuning,
    file: &Path,
) -> Result<(), Fail> {
    let cases: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(file)?)?;
    let mut wrong = 0;
    let mut changed = 0;
    for case in &cases {
        let text = |k: &str| case[k].as_str().unwrap_or_default();
        let ask = || {
            score(
                client,
                model,
                tuning,
                text("personality"),
                text("said"),
                text("text"),
            )
        };
        let first = ask().await?;
        let second = ask().await?;
        if second != first {
            changed += 1;
            eprintln!("CHANGED {}: {first:?} then {second:?}", text("name"));
        }
        let miss = |want: &str, got: u8| match want {
            "high" => got < 4,
            "low" => got > 2,
            _ => false,
        };
        let breaks_wrong = case["breaks"].as_bool().is_some_and(|b| b != first.breaks);
        if miss(text("stance"), first.stance) || breaks_wrong {
            wrong += 1;
            eprintln!("WRONG {}: {first:?} (want {case})", text("name"));
        }
    }
    println!(
        "judge fixtures: {} cases, {wrong} wrong, {changed} changed on the second run",
        cases.len()
    );
    if wrong > 0 || changed > 0 {
        return Err("the judge failed its fixtures".into());
    }
    Ok(())
}
