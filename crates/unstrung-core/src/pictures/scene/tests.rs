use super::*;

#[test]
fn a_variation_replaces_the_framing_where_it_is() {
    let p = "Anime, medium shot. A tavern. Only one woman. The woman: red hair";
    let next = varied(p, 1);
    assert!(!next.contains("medium shot"), "{next}");
    assert!(
        next.starts_with("Anime, ")
            && next.ends_with(". A tavern. Only one woman. The woman: red hair"),
        "{next}"
    );
    // "medium shots" is not the framing "medium shot".
    assert!(varied("two medium shots", 1).starts_with("two medium shots, "));
}

#[test]
fn a_variation_changes_the_framing_and_keeps_one() {
    let first = varied("anime, a tavern, fully clothed", 3);
    let framing = |p: &str| p.split(", ").filter(|x| FRAMINGS.contains(x)).count();
    assert_eq!(framing(&first), 1);
    assert!(first.starts_with("anime, a tavern, fully clothed, "));
    for seed in 0..40 {
        let next = varied(&first, seed);
        assert_eq!(framing(&next), 1);
        assert_ne!(
            first.rsplit(", ").next(),
            next.rsplit(", ").next(),
            "{next}"
        );
        assert!(next.contains("fully clothed"));
    }
}

#[test]
fn reads_a_scene_and_cleans_it() {
    let answer = "Sure! {\"setting\": \"a **rainy** street\\nat night\", \"action\": \"the character hands the user an umbrella\", \"mood\": \"warm\", \"camera\": \"medium shot\", \"who\": \"both\", \"sexual\": false}";
    let s = parse(answer).unwrap();
    assert_eq!(s.setting, "a rainy street at night");
    assert_eq!(s.who, Who::Both);
    assert!(!s.sexual);
    assert!(parse("no json here").is_none());
    assert!(parse("{\"setting\": \"\", \"action\": \"\", \"mood\": \"\", \"camera\": \"\", \"who\": \"both\"}").is_none());
    assert!(parse("{\"setting\": \"x\", \"action\": \"y\", \"mood\": \"\", \"camera\": \"\", \"who\": \"everyone\"}").is_none());
}

#[test]
fn long_fields_are_cut_at_a_word() {
    let long = "word ".repeat(100);
    let s = fallback(&long, "");
    assert!(s.action.chars().count() <= MAX_FIELD);
    assert!(s.action.ends_with("word"));
}

#[test]
fn the_prompt_uses_the_tags_of_the_people_in_the_scene() {
    let character = Person {
        appearance: "woman, silver hair, red eyes",
        outfit: "black coat",
        picture: false,
    };
    let persona = Person {
        appearance: "man, short brown hair",
        outfit: "green jacket",
        picture: false,
    };
    let mut scene = parse("{\"setting\": \"a tavern\", \"action\": \"The character pours tea for the user.\", \"mood\": \"calm\", \"camera\": \"close-up\", \"who\": \"character\", \"sexual\": false}").unwrap();
    // The place and the action come first, the looks last. The user is
    // not in the picture, so the engine is not told to draw him.
    let p = positive(&scene, "anime", &character, Some(&persona));
    assert_eq!(
        p,
        "Anime, close-up. Only one woman. A tavern. The woman pours tea for someone out of view. Calm mood. The woman has silver hair, red eyes and wears black coat"
    );
    scene.who = Who::Both;
    let p = positive(&scene, "", &character, Some(&persona));
    assert_eq!(
        p,
        "Close-up. Exactly two people, a woman and a man. A tavern. The woman pours tea for the man. Calm mood. The woman has silver hair, red eyes and wears black coat. The man has short brown hair and wears green jacket"
    );
    scene.who = Who::Persona;
    // No persona in the chat: nobody is drawn in the user's place.
    assert!(!positive(&scene, "", &character, None).contains("silver hair"));
    scene.who = Who::Nobody;
    let p = positive(&scene, "", &character, Some(&persona));
    assert!(!p.contains("hair") && !p.contains("woman"), "{p}");
}

#[test]
fn people_of_one_kind_are_told_apart() {
    let a = Person {
        appearance: "man, red beard",
        outfit: "",
        picture: false,
    };
    let b = Person {
        appearance: "Man, bald",
        outfit: "",
        picture: false,
    };
    let mut scene = fallback("the character shakes hands with the user", "");
    scene.who = Who::Both;
    let p = positive(&scene, "", &a, Some(&b));
    assert!(
        p.contains("The first man shakes hands with the second man"),
        "{p}"
    );
    assert!(
        p.contains("Exactly two men.")
            && p.contains("The first man has red beard. The second man has bald"),
        "{p}"
    );
    // No kind in the Looks: person.
    let c = Person {
        appearance: "red beard",
        outfit: "",
        picture: false,
    };
    let p = positive(&scene, "", &c, Some(&b));
    assert!(p.contains("The person shakes hands with the man"), "{p}");
    // Only whole words change.
    assert_eq!(
        replace_words("The users wait. THE USER waves.", "the user", "the man"),
        "The users wait. the man waves."
    );
}

#[test]
fn clothes_from_the_story_replace_the_usual_outfit_of_that_person() {
    let character = Person {
        appearance: "silver hair",
        outfit: "black coat",
        picture: false,
    };
    let persona = Person {
        appearance: "brown hair",
        outfit: "green jacket",
        picture: false,
    };
    let mut scene = fallback("they dance", "");
    scene.who = Who::Both;
    // No change in the story: the usual outfits.
    let p = positive(&scene, "", &character, Some(&persona));
    assert!(
        p.contains("first person has silver hair and wears black coat"),
        "{p}"
    );
    assert!(
        p.contains("second person has brown hair and wears green jacket"),
        "{p}"
    );
    // Each person gets their own new clothes.
    scene.character_outfit = "a red evening dress".into();
    scene.persona_outfit = "a grey suit".into();
    let p = positive(&scene, "", &character, Some(&persona));
    assert!(
        p.contains("first person has silver hair and wears a red evening dress"),
        "{p}"
    );
    assert!(
        p.contains("second person has brown hair and wears a grey suit"),
        "{p}"
    );
    assert!(
        !p.contains("black coat") && !p.contains("green jacket"),
        "{p}"
    );
    // A story outfit that only drops words of the usual one keeps it.
    scene.persona_outfit = "his jacket".into();
    let p = positive(&scene, "", &character, Some(&persona));
    assert!(p.contains("brown hair and wears green jacket"), "{p}");
    // Only the character changed.
    scene.persona_outfit.clear();
    let p = positive(&scene, "", &character, Some(&persona));
    assert!(p.contains("brown hair and wears green jacket"), "{p}");
    // With no persona nobody is drawn, and the user's clothes never
    // dress the character.
    scene.who = Who::Persona;
    scene.character_outfit.clear();
    scene.persona_outfit = "a grey suit".into();
    let p = positive(&scene, "", &character, None);
    assert!(
        !p.contains("silver hair") && !p.contains("grey suit"),
        "{p}"
    );
}

#[test]
fn only_clothes_that_the_chat_named_stay() {
    let mut scene = fallback("x", "");
    scene.character_outfit = "a long red evening dress".into();
    scene.persona_outfit = "work overalls".into();
    keep_said_outfits(
        &mut scene,
        "She comes back in a long red evening dress. You look amazing.",
    );
    assert_eq!(scene.character_outfit, "a long red evening dress");
    assert!(scene.persona_outfit.is_empty());

    let mut scene = fallback("x", "");
    scene.character_outfit = "utility jumpsuit".into();
    keep_said_outfits(&mut scene, "Then we make a new one. Grab a wrench.");
    assert!(scene.character_outfit.is_empty());

    // Plurals and pieces: one named piece is enough.
    let mut scene = fallback("x", "");
    scene.character_outfit = "a grey suit with a silk tie".into();
    keep_said_outfits(&mut scene, "He puts on his best suits for the trial.");
    assert_eq!(scene.character_outfit, "a grey suit with a silk tie");
}

#[test]
fn an_old_answer_without_outfits_still_reads() {
    let scene =
        parse(r#"{"setting": "a tavern", "action": "x", "mood": "", "camera": "", "who": "both"}"#)
            .unwrap();
    assert!(scene.character_outfit.is_empty() && scene.persona_outfit.is_empty());
}

#[test]
fn the_rules_steer_both_prompts() {
    let sfw = with_rules(
        "a park",
        Rules {
            no_nudity: true,
            adults_only: false,
            mature: false,
        },
    );
    assert!(sfw.positive.ends_with("fully clothed"));
    assert!(sfw.negative.contains("nude"));
    let adult = with_rules(
        "a park",
        Rules {
            no_nudity: false,
            adults_only: true,
            mature: false,
        },
    );
    assert!(adult.positive.ends_with("adults"));
    assert!(adult.negative.contains("child"));
}

#[test]
fn the_question_carries_the_wish_and_no_names_rule() {
    let lines = vec![("Ren".to_owned(), "Sit down.".to_owned())];
    let q = question("Ren", "Alex", &lines, "us on the roof");
    assert!(q.contains("Ren: Sit down."));
    assert!(q.contains("never names"));
    assert!(q.contains("The user wants this picture: us on the roof"));
}

#[test]
fn a_saved_prompt_knows_its_people_and_no_persona_means_nobody() {
    let kai = Person {
        appearance: "man, messy brown hair, stubble",
        outfit: "olive jacket",
        picture: true,
    };
    let mut scene = fallback("the user waves", "");
    scene.who = Who::Persona;
    let text = positive(&scene, "", &kai, Some(&kai));
    assert!(describes(&text, kai.appearance), "{text}");
    assert!(!describes(&text, "woman, wavy brown hair, round glasses"));
    // A persona with only a picture and a name: no proof, so no picture.
    assert!(!describes(&text, ""));
    assert!(!describes(&text, "woman"));
    // No persona in the chat: nobody, not the character in the user's place.
    assert!(references(&scene, &kai, None).is_empty());
    assert!(!positive(&scene, "", &kai, None).contains("stubble"));
}

#[test]
fn image_numbers_follow_the_people_with_a_picture() {
    let with = |picture| Person {
        appearance: "silver hair",
        outfit: "coat",
        picture,
    };
    let mut scene = fallback("the character hands the user a cup", "");
    scene.who = Who::Both;
    // Both pictures: image 1 is the character, image 2 the persona, each
    // named where the action first mentions them.
    let (c, p) = (with(true), with(true));
    let text = positive(&scene, "", &c, Some(&p));
    assert!(
        text.contains("The first person from image 1 hands the second person from image 2 a cup"),
        "{text}"
    );
    assert!(text.contains("The first person has silver hair"), "{text}");
    assert_eq!(
        references(&scene, &c, Some(&p)),
        vec![Who::Character, Who::Persona]
    );
    // Only the persona has one: it is image 1.
    let (c, p) = (with(false), with(true));
    let text = positive(&scene, "", &c, Some(&p));
    assert!(
        text.contains("The first person hands the second person from image 1 a cup"),
        "{text}"
    );
    assert_eq!(references(&scene, &c, Some(&p)), vec![Who::Persona]);
    // Only the character.
    let (c, p) = (with(true), with(false));
    assert_eq!(references(&scene, &c, Some(&p)), vec![Who::Character]);
    // Neither: no image words.
    let (c, p) = (with(false), with(false));
    assert!(!positive(&scene, "", &c, Some(&p)).contains("image"));
    assert!(references(&scene, &c, Some(&p)).is_empty());
    // Not in the action: the link goes to the looks.
    let mut quiet = fallback("rain falls", "");
    quiet.who = Who::Character;
    let c = with(true);
    let text = positive(&quiet, "", &c, None);
    assert!(text.contains("Only one person. Rain falls"), "{text}");
    assert!(
        text.contains("The person from image 1 has silver hair"),
        "{text}"
    );
    assert_eq!(references(&quiet, &c, None), vec![Who::Character]);
    // Without the pictures, the prompt is the tags-only one.
    assert_eq!(
        without_references(&text),
        positive(&quiet, "", &with(false), None)
    );
    let (c, p) = (with(true), with(true));
    assert_eq!(
        without_references(&positive(&scene, "", &c, Some(&p))),
        positive(&scene, "", &with(false), Some(&with(false)))
    );
    // A person with a picture and no looks keeps the link.
    let bare = Person {
        appearance: "",
        outfit: "",
        picture: true,
    };
    let text = positive(&quiet, "", &bare, None);
    assert!(
        text.ends_with("The person from image 1 is in the scene"),
        "{text}"
    );
}
