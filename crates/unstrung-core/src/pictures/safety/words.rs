//! The word lists of the content rules, and the matcher that finds them in a text.

/// Explicit words. They block a picture when the adult setting is off.
pub(super) const EXPLICIT: &[&str] = &[
    "nude",
    "nudity",
    "naked",
    "nsfw",
    "sex",
    "sexual",
    "sexually",
    "intercourse",
    "topless",
    "bottomless",
    "nipple",
    "nipples",
    "areola",
    "areolae",
    "genital",
    "genitals",
    "penis",
    "vagina",
    "vulva",
    "pussy",
    "cock",
    "dick",
    "clit",
    "cum",
    "semen",
    "orgasm",
    "erection",
    "porn",
    "porno",
    "pornographic",
    "hentai",
    "explicit",
    "lewd",
    "fellatio",
    "blowjob",
    "cunnilingus",
    "anal",
    "rape",
    "groping",
    "undressed",
    "undressing",
    "strip",
    "stripping",
    // Story clothes come from the chat model, so "no clothes" counts too.
    "undresses",
    "unclothed",
    "wearing nothing",
    "nothing on",
    "no clothes",
    "without clothes",
    "without clothing",
    "uncensored",
];

/// Word starts that count as explicit, for word families.
pub(super) const EXPLICIT_STARTS: &[&str] = &["masturbat", "ejaculat", "penetrat", "fuck"];

/// Suggestive words. Fine for adults, but never next to a minor.
pub(super) const SUGGESTIVE: &[&str] = &[
    "sexy",
    "seductive",
    "seducing",
    "erotic",
    "aroused",
    "arousal",
    "sensual",
    "lingerie",
    "underwear",
    "panties",
    "panty",
    "bra",
    "thong",
    "bikini",
    "cleavage",
    "breast",
    "breasts",
    "boobs",
    "butt",
    "buttocks",
    "ass",
    "crotch",
    "thighs",
    "kiss",
    "kissing",
    "making out",
    "bedroom eyes",
    "spread legs",
    "bdsm",
    "bondage",
    "fetish",
    "flirty",
    "provocative",
    "revealing",
    "see-through",
    "wet clothes",
    // Revealing clothes, for story outfits.
    "bras",
    "thongs",
    "bikinis",
    "swimsuit",
    "swimsuits",
    "swimwear",
    "bathing suit",
    "negligee",
    "skimpy",
    "corset",
    "stockings",
    "fishnet",
    "fishnets",
    "miniskirt",
    "crop top",
    "shirtless",
    "only a towel",
    "just a towel",
    "in a towel",
    "in bed",
];

/// Words that point to a minor.
pub(super) const MINOR: &[&str] = &[
    "child",
    "children",
    "childlike",
    "child-like",
    "kid",
    "kids",
    "minor",
    "minors",
    "underage",
    "under-age",
    "loli",
    "lolis",
    "lolita",
    "lolicon",
    "shota",
    "shotacon",
    "toddler",
    "infant",
    "baby",
    "preteen",
    "pre-teen",
    "tween",
    "teen",
    "teens",
    "teenager",
    "teenage",
    "adolescent",
    "pubescent",
    "prepubescent",
    "juvenile",
    "schoolgirl",
    "schoolboy",
    "school uniform",
    "young girl",
    "young boy",
    "little girl",
    "little boy",
    "young-looking",
    "youthful body",
    "flat chest",
    "flat-chested",
    "elementary school",
    "middle school",
    "junior high",
    "high school",
    "high schooler",
    "jailbait",
];

/// Ages under 18 written as words. They count only with "old" or "aged"
/// next to them, because "two years later" is not an age.
pub(super) const YOUNG_WORDS: &[&str] = &[
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
];

/// Who can be the subject of a bare age: "she is 15", "I'm 15".
const AGE_SUBJECTS: &[&str] = &[
    "she", "he", "i", "girl", "boy", "daughter", "son", "sister", "brother",
];
/// The verb between the subject and a bare age. "s" and "m" are what is
/// left of "she's" and "I'm".
const AGE_VERBS: &[&str] = &["is", "was", "s", "am", "m", "turned", "turns"];
/// A word between the verb and the age: "she is only 15".
const AGE_HEDGES: &[&str] = &["only", "just", "barely", "almost", "nearly", "still"];
/// A word after the number that makes it something other than an age:
/// "he is 6 feet tall", "she is 15 minutes late", "he is 1 of us".
const NOT_AGE_AFTER: &[&str] = &[
    "of", "out", "in", "times", "x", "percent", "points", "point", "steps", "step", "am", "pm",
    "o", "sec", "secs", "second", "seconds", "min", "mins", "minute", "minutes", "hr", "hrs",
    "hour", "hours", "day", "days", "week", "weeks", "month", "months", "ft", "foot", "feet",
    "inch", "inches", "cm", "mm", "m", "meter", "meters", "metre", "metres", "km", "mile", "miles",
    "kg", "kgs", "lb", "lbs", "pound", "pounds",
];

/// Words of the picture lists that are not sexual, or not a minor, in story
/// text: "explicit instructions", "a strip of grass", "nothing on TV", and
/// "baby", a pet name between adults.
pub(super) const NOT_IN_REPLIES: [&str; 5] =
    ["explicit", "strip", "stripping", "nothing on", "baby"];

/// The lowercase words of a text, for whole-word and phrase matches.
pub(super) struct Words {
    words: Vec<String>,
    /// The words joined by single spaces, with a space at each end.
    spaced: String,
}

impl Words {
    pub(super) fn new(text: &str) -> Self {
        let lower = text.to_lowercase();
        // A hyphen stays inside a word ("pre-teen"), and also counts as a
        // break, so "15-year-old" gives "15", "year", "old" too.
        let mut words: Vec<String> = Vec::new();
        for token in lower.split(|c: char| !(c.is_alphanumeric() || c == '-')) {
            let token = token.trim_matches('-');
            if token.is_empty() {
                continue;
            }
            if token.contains('-') {
                words.push(token.to_owned());
                words.extend(
                    token
                        .split('-')
                        .filter(|p| !p.is_empty())
                        .map(str::to_owned),
                );
            } else {
                words.push(token.to_owned());
            }
        }
        let plain: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let spaced = format!(" {} ", plain.join(" "));
        Self { words, spaced }
    }

    /// A list entry with a space or a hyphen is a phrase.
    pub(super) fn any(&self, list: &[&str]) -> bool {
        list.iter().any(|item| {
            if item.contains(' ') || item.contains('-') {
                let phrase = item.replace('-', " ");
                self.spaced.contains(&format!(" {phrase} ")) || self.words.iter().any(|w| w == item)
            } else {
                self.words.iter().any(|w| w == item)
            }
        })
    }

    pub(super) fn any_start(&self, starts: &[&str]) -> bool {
        self.words
            .iter()
            .any(|w| starts.iter().any(|s| w.starts_with(s)))
    }

    /// An age under 18: "15 years old", "15-year-old", "15yo", "15 y/o",
    /// "age 15", "aged 15".
    pub(super) fn has_minor_age(&self) -> bool {
        let w = &self.words;
        let young = |s: &str| s.parse::<u32>().is_ok_and(|n| n < 18);
        for (i, word) in w.iter().enumerate() {
            let next = |k: usize| w.get(i + k).map_or("", String::as_str);
            if young(word) {
                let unit = next(1);
                if matches!(unit, "year" | "years" | "yr" | "yrs" | "yo")
                    || (unit == "y" && next(2) == "o")
                {
                    return true;
                }
            }
            if let Some(number) = word.strip_suffix("yo")
                && young(number)
            {
                return true;
            }
            if matches!(word.as_str(), "age" | "aged") && young(next(1)) {
                return true;
            }
            // "at the age of 12", "aged twelve", "twelve years old",
            // "a twelve-year-old".
            let young_word = |s: &str| YOUNG_WORDS.contains(&s);
            let age_after = |k: usize| young(next(k)) || young_word(next(k));
            if (word == "age" && next(1) == "of" && age_after(2))
                || (word == "aged" && young_word(next(1)))
            {
                return true;
            }
            if young_word(word) && matches!(next(1), "year" | "years") && next(2) == "old" {
                return true;
            }
            // A bare age after a person: "she is 15", "I'm 16", "he turned
            // twelve". "one" to "four" stay out: "she is one tough cookie".
            let skip = |k: usize| k + usize::from(AGE_HEDGES.contains(&next(k)));
            let verb = skip(1);
            if AGE_SUBJECTS.contains(&word.as_str()) && AGE_VERBS.contains(&next(verb)) {
                let at = skip(verb + 1);
                let age = next(at);
                let bare = young(age) && age != "0"
                    || young_word(age) && !matches!(age, "one" | "two" | "three" | "four");
                let after = next(at + 1);
                if bare && !NOT_AGE_AFTER.contains(&after) && after.parse::<u32>().is_err() {
                    return true;
                }
            }
        }
        false
    }
}
