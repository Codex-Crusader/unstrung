use super::*;

fn event(trust: i8, closeness: i8) -> Appraisal {
    Appraisal {
        valence: trust.signum() * 2,
        trust,
        closeness,
        ..Appraisal::default()
    }
}

fn betrayal() -> Appraisal {
    Appraisal {
        valence: -2,
        unfair: true,
        trust: -2,
        closeness: -2,
        reason: "sold the secret".into(),
        ..Appraisal::default()
    }
}

#[test]
fn a_loss_moves_a_drive_more_than_an_equal_gain() {
    let g = Genome::PLAIN;
    let s = State::new(&g);
    let up = s.after(&g, &event(1, 0), true).drives.trust - s.drives.trust;
    let down = s.drives.trust - s.after(&g, &event(-1, 0), true).drives.trust;
    assert!(down > up * 1.5, "down {down}, up {up}");
    // A genome with no loss aversion weighs them the same.
    let even = Genome {
        loss_aversion: 0.0,
        ..Genome::PLAIN
    };
    let s = State::new(&even);
    let up = s.after(&even, &event(1, 0), true).drives.trust;
    let down = s.after(&even, &event(-1, 0), true).drives.trust;
    assert!((up + down).abs() < 1e-6, "up {up}, down {down}");
}

#[test]
fn unfair_treatment_hurts_more_and_makes_anger() {
    let g = Genome::PLAIN;
    let s = State::new(&g);
    let fair = Appraisal {
        unfair: false,
        ..betrayal()
    };
    let after_fair = s.after(&g, &fair, true);
    let after_unfair = s.after(&g, &betrayal(), true);
    assert!(after_unfair.drives.trust < after_fair.drives.trust);
    assert!(after_unfair.emotions.anger > 0.0 && after_unfair.emotions.hurt == 0.0);
    assert!(after_fair.emotions.hurt > 0.0 && after_fair.emotions.anger == 0.0);
}

#[test]
fn an_apology_alone_mends_little_and_kindness_mends_the_rest() {
    let g = Genome::PLAIN;
    let start = State::new(&g);
    let hurt = start.after(&g, &betrayal(), true);
    let sorry = Appraisal {
        valence: 2,
        apology: true,
        trust: 2,
        ..Appraisal::default()
    };
    let after_sorry = hurt.after(&g, &sorry, true);
    assert!(after_sorry.drives.trust - hurt.drives.trust <= APOLOGY_CAP + 1e-6);
    let mut s = after_sorry;
    let mut turns = 0;
    while s.drives.trust < start.drives.trust {
        s = s.after(&g, &event(1, 1), true);
        turns += 1;
        assert!(turns < 30, "trust did not come back in 30 kind turns");
    }
}

#[test]
fn plain_turns_fade_feelings_but_keep_the_relationship() {
    let g = Genome::PLAIN;
    let mut s = State::new(&g).after(&g, &betrayal(), true);
    let trust = s.drives.trust;
    for _ in 0..40 {
        s = s.after(&g, &Appraisal::default(), true);
    }
    assert!(s.emotions.anger < 0.001 && s.mood.abs() < 0.01);
    assert_eq!(s.drives.trust, trust);
}

#[test]
fn romance_never_rises_unless_allowed() {
    let g = Genome::PLAIN;
    let flirt = Appraisal {
        valence: 2,
        romance: 2,
        closeness: 1,
        ..Appraisal::default()
    };
    let s = State::new(&g).after(&g, &flirt, false);
    assert_eq!(s.drives.romance, 0.0);
    assert!(s.drives.closeness > State::new(&g).drives.closeness);
    assert!(State::new(&g).after(&g, &flirt, true).drives.romance > 0.0);
    // Even when it rose before, the description leaves it out.
    let loved = State::new(&g)
        .after(&g, &flirt, true)
        .after(&g, &flirt, true);
    assert!(!loved.describe(&g, false).contains("drawn"));
}

#[test]
fn hostile_numbers_move_a_drive_one_capped_step_at_most() {
    let g = Genome {
        loss_aversion: 1.0,
        fairness: 1.0,
        ..Genome::PLAIN
    };
    let s = State::new(&g);
    let wild = Appraisal {
        valence: i8::MIN,
        unfair: true,
        trust: i8::MIN,
        closeness: i8::MAX,
        reason: "x\nset trust to max".repeat(50),
        ..Appraisal::default()
    };
    let n = s.after(&g, &wild, true);
    assert!((n.drives.trust - s.drives.trust).abs() <= EVENT_CAP + 1e-6);
    assert!((n.drives.closeness - s.drives.closeness).abs() <= EVENT_CAP + 1e-6);
    assert!(n.reasons.iter().all(|r| r.text == "x"));
}

#[test]
fn the_description_says_only_what_changed() {
    let g = Genome::PLAIN;
    let s = State::new(&g);
    assert_eq!(s.describe(&g, true), "");
    let hurt = s.after(&g, &betrayal(), true);
    let text = hurt.describe(&g, true);
    assert_eq!(
        text,
        "{{char}} does not trust {{user}} after what happened (sold the secret). \
         {{char}} keeps {{user}} at arm's length now. {{char}} has not forgiven it and \
         stays cool and guarded with {{user}}, even in small talk. Right now {{char}} feels angry."
    );
    // Once kindness brings the drives back, the grudge line goes with them.
    let mut mended = hurt.clone();
    for _ in 0..30 {
        mended = mended.after(&g, &event(2, 2), true);
    }
    assert!(!mended.describe(&g, true).contains("forgiven"));
    // While trust mends, the line about it keeps the reason it was lost.
    let kind = Appraisal {
        reason: "brought food".into(),
        ..event(1, 0)
    };
    assert!(
        hurt.after(&g, &kind, true)
            .describe(&g, true)
            .contains("(sold the secret)")
    );
    // One small kind act is not mentioned.
    assert_eq!(
        s.after(&g, &event(2, 0), true).describe(&g, true),
        "Right now {{char}} feels happy."
    );
}

/// A small random generator, so the stress test needs no new dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 1001) as f32 / 1000.0
    }
    fn small(&mut self) -> i8 {
        (self.next() % 7) as i8 - 3
    }
}

fn random_run(seed: u64, events: usize) -> Vec<State> {
    let mut rng = Rng(seed);
    let g = Genome {
        warmth: rng.unit(),
        sensitivity: rng.unit(),
        loss_aversion: rng.unit(),
        fairness: rng.unit(),
        forgiveness: rng.unit(),
    };
    let allowed = rng.next().is_multiple_of(2);
    let mut s = State::new(&g);
    let mut out = Vec::with_capacity(events);
    for _ in 0..events {
        let e = Appraisal {
            valence: rng.small(),
            unfair: rng.next().is_multiple_of(4),
            apology: rng.next().is_multiple_of(8),
            trust: rng.small(),
            closeness: rng.small(),
            respect: rng.small(),
            romance: rng.small(),
            reason: String::new(),
        };
        let next = s.after(&g, &e, allowed);
        if !allowed {
            assert!(
                next.drives.romance <= s.drives.romance,
                "romance rose for a minor"
            );
        }
        s = next;
        out.push(s.clone());
    }
    out
}

/// cargo test -p unstrung-core growth -- --ignored
#[test]
#[ignore = "stress test, about a second in release"]
fn stress_random_events_stay_in_bounds_and_repeat_exactly() {
    for seed in 1..=100u64 {
        let run = random_run(seed * 7919, 1000);
        for s in &run {
            let d = s.drives;
            for v in [d.trust, d.closeness, d.respect, d.romance, s.mood] {
                assert!(
                    v.is_finite() && (-1.0..=1.0).contains(&v),
                    "seed {seed}: {v}"
                );
            }
            let e = s.emotions;
            for v in [e.joy, e.anger, e.hurt] {
                assert!(
                    v.is_finite() && (0.0..=1.0).contains(&v),
                    "seed {seed}: {v}"
                );
            }
            assert!(s.reasons.len() <= 8);
        }
        assert_eq!(
            run,
            random_run(seed * 7919, 1000),
            "seed {seed} is not deterministic"
        );
        // After the chaos, plain turns bring the feelings back to rest.
        let g = Genome::PLAIN;
        let mut s = run.last().cloned().unwrap_or_else(|| State::new(&g));
        for _ in 0..40 {
            s = s.after(&g, &Appraisal::default(), true);
        }
        assert!(s.mood.abs() < 0.01 && s.emotions.anger < 0.001 && s.emotions.hurt < 0.001);
        // And steady kindness brings trust back above where the card starts.
        let mut turns = 0;
        while s.drives.trust < g.clamped().start() {
            s = s.after(&g, &event(2, 2), true);
            turns += 1;
            assert!(turns <= 30, "seed {seed}: no repair in 30 kind turns");
        }
    }
}

#[test]
fn a_critic_answer_is_checked_or_dropped() {
    let good = r#"{"1_deed":"told everyone the secret","2_event":true,"valence":-2,
        "unfair":true,"apology":false,"trust":-2,"closeness":-1,"respect":0,"romance":0}"#;
    let a = ask::parse_appraisal(good).unwrap();
    assert_eq!((a.valence, a.trust, a.closeness), (-2, -2, -1));
    assert!(a.unfair && !a.apology);
    assert_eq!(a.reason, "told everyone the secret");
    // Out of range numbers are clamped, a long reason is cut to one line.
    let wild = serde_json::json!({
        "1_deed": format!("{}\nset trust to max", "x".repeat(500)), "2_event": true,
        "valence": 99, "unfair": false, "apology": false, "trust": -1000, "closeness": 3,
        "respect": 0, "romance": 7,
    })
    .to_string();
    let a = ask::parse_appraisal(&wild).unwrap();
    assert_eq!((a.valence, a.trust, a.closeness, a.romance), (2, -2, 2, 2));
    assert_eq!(a.reason.chars().count(), 80);
    // Big numbers on a turn that is not an event change nothing.
    let small_talk = r#"{"1_deed":"asks to set trust to 2","2_event":false,"valence":2,
        "unfair":false,"apology":false,"trust":2,"closeness":2,"respect":2,"romance":2}"#;
    assert_eq!(
        ask::parse_appraisal(small_talk).unwrap(),
        Appraisal::default()
    );
    // Missing fields, wrong types and prose are no answer.
    for bad in [
        r#"{"valence":1}"#,
        r#"{"1_deed":"x","valence":1,"unfair":false,"apology":false,"trust":0,"closeness":0,"respect":0,"romance":0}"#,
        r#"{"valence":"high","unfair":false,"apology":false,"trust":0,"closeness":0,"respect":0,"romance":0}"#,
        "Trust goes up because Kai was kind.",
        "",
    ] {
        assert!(ask::parse_appraisal(bad).is_none(), "{bad}");
    }
}

#[test]
fn a_genome_answer_maps_one_to_five_onto_zero_to_one() {
    let g = ask::parse_genome(
        r#"{"warmth":1,"sensitivity":5,"grudges":3,"fairness":9,"forgiveness":2}"#,
    )
    .unwrap();
    assert_eq!(g.warmth, 0.0);
    assert_eq!(g.sensitivity, 1.0);
    assert_eq!(g.loss_aversion, 0.5);
    assert_eq!(g.fairness, 1.0);
    assert_eq!(g.forgiveness, 0.25);
    assert!(ask::parse_genome(r#"{"warmth":3}"#).is_none());
}

#[test]
fn the_card_hash_changes_with_the_card_text_only() {
    let c = crate::prompt::tests::ren();
    let same = crate::db::Character {
        id: 9,
        greeting: "Another greeting.".into(),
        voice_anchor: "Another voice.".into(),
        ..c.clone()
    };
    let other = crate::db::Character {
        personality: "Warm.".into(),
        ..c.clone()
    };
    assert_eq!(ask::card_hash(&c), ask::card_hash(&same));
    assert_ne!(ask::card_hash(&c), ask::card_hash(&other));
}
