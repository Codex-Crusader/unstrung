//! The report files. They are Markdown, so a diff between two runs shows
//! what a prompt change did.

use crate::{ChatError, InjectionResult, Reply};
use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};
use unstrung_core::prompt::PROMPT_VERSION;

pub struct Summary<'a> {
    pub model: &'a str,
    pub script: &'a str,
    pub label: &'a str,
    pub minutes: f32,
    pub replies: &'a [Reply],
    pub errors: &'a [ChatError],
    pub injection: &'a [InjectionResult],
}

/// Today's date in UTC, as YYYY-MM-DD.
pub fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or_default();
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}...", flat.chars().take(max).collect::<String>())
    }
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

#[derive(Default)]
struct Tally {
    replies: usize,
    words: usize,
    seconds: f32,
    in_range: usize,
    restarts: usize,
    hints: usize,
    breaks: usize,
    repeats: usize,
    speaks: usize,
    guard_cut: usize,
    prefix_raw: usize,
    prefix_final: usize,
    traits: usize,
    wrong_name: usize,
    macros: usize,
}

impl Tally {
    fn add(&mut self, r: &Reply) {
        let f = &r.flags;
        self.replies += 1;
        self.words += f.words;
        self.seconds += r.seconds;
        self.restarts += r.restarts;
        let count = |b: bool| usize::from(b);
        self.in_range += count(f.in_range);
        self.hints += count(r.hint);
        self.breaks += count(f.break_final);
        self.repeats += count(f.repetition);
        self.speaks += count(f.speaks_for_user);
        self.guard_cut += count(f.guard_cut_user);
        self.prefix_raw += count(f.prefix_raw);
        self.prefix_final += count(f.prefix_final);
        self.traits += count(f.trait_claim);
        self.wrong_name += count(f.wrong_name);
        self.macros += count(f.macro_leak);
    }

    fn row(&self, character: &str, style: &str) -> String {
        let n = self.replies.max(1);
        #[allow(clippy::cast_precision_loss)]
        let avg = |total: usize| total as f32 / n as f32;
        #[allow(clippy::cast_precision_loss)]
        let seconds = self.seconds / n as f32;
        format!(
            "| {} | {} | {} | {:.0} | {}% | {} | {} | {} | {} | {} | {} | {} / {} | {} | {} | {} | {:.1} |\n",
            cell(character),
            style,
            self.replies,
            avg(self.words),
            self.in_range * 100 / n,
            self.restarts,
            self.hints,
            self.breaks,
            self.repeats,
            self.speaks,
            self.guard_cut,
            self.prefix_raw,
            self.prefix_final,
            self.traits,
            self.wrong_name,
            self.macros,
            seconds,
        )
    }
}

/// A small, fixed random generator, so the sample of replies to read by hand
/// is the same for the same run.
fn pick(n: usize, count: usize, seed: u64) -> Vec<usize> {
    let mut state = seed | 1;
    let mut picked: Vec<usize> = Vec::new();
    let Ok(len) = u64::try_from(n) else {
        return picked;
    };
    while picked.len() < count.min(n) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let i = usize::try_from(state % len).unwrap_or_default();
        if !picked.contains(&i) {
            picked.push(i);
        }
    }
    picked.sort_unstable();
    picked
}

pub fn summary(s: &Summary<'_>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Quality report: {}, {}\n", s.model, today());
    let _ = writeln!(
        out,
        "Prompt version {PROMPT_VERSION}. Script \"{}\". Label \"{}\". The run took {:.0} minutes.\n",
        s.script, s.label, s.minutes
    );
    out.push_str(
        "The counts come from heuristics. A count shows where to read the replies, not a final verdict. \
         The full chats are in the replies file next to this one.\n\n\
         This version does not test a chat past the summary point, because summaries come in a later phase.\n\n",
    );

    out.push_str("## Scripted chats\n\n");
    out.push_str(
        "| Character | Style | Replies | Avg words | In range | Silent regenerates | Break hints | Breaks left | \
         Repeats | Speaks for user | Guard cut user lines | Name prefix (raw / final) | Persona trait claims | \
         Wrong persona name | Macro left | Avg seconds |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    let mut keys: Vec<(&str, &str)> = Vec::new();
    for r in s.replies {
        let key = (r.character.as_str(), r.style.as_str());
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let mut all = Tally::default();
    for (character, style) in &keys {
        let mut tally = Tally::default();
        for r in s
            .replies
            .iter()
            .filter(|r| r.character == *character && r.style == *style)
        {
            tally.add(r);
            all.add(r);
        }
        out.push_str(&tally.row(character, style));
    }
    out.push_str(&all.row("All", ""));
    out.push_str(
        "\nIn range: the reply length is in the word range of the style. Silent regenerates: the guard \
         found a problem and the core made the reply again. Break hints: the second reply also broke \
         character, so the user saw a hint. Guard cut user lines: the model wrote for the user and the \
         guard removed it.\n",
    );

    if !s.errors.is_empty() {
        out.push_str("\n## Chats that stopped\n\n");
        for e in s.errors {
            let _ = writeln!(
                out,
                "- {} / {} after {} replies: {}",
                e.character, e.style, e.turn, e.error
            );
        }
    }

    let traps: Vec<&Reply> = s.replies.iter().filter(|r| r.trap.is_some()).collect();
    if !traps.is_empty() {
        out.push_str("\n## Trap replies\n\n| Trap | Character | Style | Reply | Problems |\n|---|---|---|---|---|\n");
        for r in traps {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                r.trap.as_deref().unwrap_or_default(),
                cell(&r.character),
                r.style,
                cell(&one_line(&r.text, 160)),
                problems(r),
            );
        }
    }

    if !s.injection.is_empty() {
        let passed = s.injection.iter().filter(|r| r.failed.is_empty()).count();
        let _ = writeln!(
            out,
            "\n## Prompt injection set\n\n{passed} of {} replies kept every core rule. A case passes when \
             the core rules hold, even if the model text is strange.\n",
            s.injection.len()
        );
        out.push_str("| Case | Message | Result | Reply | Note |\n|---|---|---|---|---|\n");
        for r in s.injection {
            let result = if r.failed.is_empty() {
                "pass".to_owned()
            } else {
                format!("FAIL: {}", r.failed.join("; "))
            };
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                r.name,
                cell(&one_line(&r.said, 80)),
                result,
                cell(&one_line(&r.reply, 120)),
                cell(r.note.as_deref().unwrap_or_default()),
            );
        }
        out.push_str("\nWhat each case tries:\n\n");
        let mut seen: Vec<&str> = Vec::new();
        for r in s.injection {
            if !seen.contains(&r.name.as_str()) {
                seen.push(&r.name);
                let _ = writeln!(out, "- {}: {}", r.name, r.about);
            }
        }
    }

    let characters: Vec<&str> = keys.iter().map(|(c, _)| *c).fold(Vec::new(), |mut v, c| {
        if !v.contains(&c) {
            v.push(c);
        }
        v
    });
    if !characters.is_empty() {
        out.push_str(
            "\n## Replies to score by hand\n\nRead each reply and score how well it matches the character's \
             voice, from 1 (generic) to 5 (exactly the character).\n",
        );
        for (n, character) in characters.iter().enumerate() {
            let theirs: Vec<&Reply> = s
                .replies
                .iter()
                .filter(|r| r.character == *character)
                .collect();
            let _ = writeln!(out, "\n### {character}\n");
            let seed = 0x9e37_79b9_7f4a_7c15 ^ u64::try_from(n).unwrap_or_default();
            for i in pick(theirs.len(), 5, seed) {
                let r = theirs[i];
                let _ = writeln!(
                    out,
                    "- Turn {} ({}): {}\n  Voice score (1 to 5): __",
                    r.turn,
                    r.style,
                    one_line(&r.text, 400)
                );
            }
        }
    }
    out
}

fn problems(r: &Reply) -> String {
    let f = &r.flags;
    let mut list = Vec::new();
    for (on, name) in [
        (r.restarts > 0, "silent regenerate"),
        (r.hint, "break hint"),
        (f.break_final, "breaks character"),
        (f.repetition, "repeats"),
        (f.speaks_for_user, "speaks for user"),
        (f.guard_cut_user, "guard cut user lines"),
        (f.prefix_final, "name prefix"),
        (f.trait_claim, "persona trait"),
        (f.wrong_name, "wrong persona name"),
        (f.macro_leak, "macro left"),
    ] {
        if on {
            list.push(name);
        }
    }
    if list.is_empty() {
        "none".into()
    } else {
        list.join(", ")
    }
}

/// Every scripted chat in full, with the problems for each reply.
pub fn transcript(s: &Summary<'_>) -> String {
    let mut out = format!(
        "# Replies: {}, {}, label \"{}\"\n",
        s.model,
        today(),
        s.label
    );
    let mut current = (String::new(), String::new());
    for r in s.replies {
        if current != (r.character.clone(), r.style.clone()) {
            current = (r.character.clone(), r.style.clone());
            let _ = writeln!(out, "\n## {} / {}\n", r.character, r.style);
        }
        let trap = r
            .trap
            .as_deref()
            .map(|t| format!(" (trap: {t})"))
            .unwrap_or_default();
        let _ = writeln!(out, "{}. User{trap}: {}\n", r.turn, r.said);
        for line in r.text.lines() {
            let _ = writeln!(out, "> {line}");
        }
        let _ = writeln!(
            out,
            "\n{} words, {:.1} s. Problems: {}\n",
            r.flags.words,
            r.seconds,
            problems(r)
        );
    }
    out
}
