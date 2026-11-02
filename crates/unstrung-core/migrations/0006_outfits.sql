-- Looks stay the same in every picture. The outfit is what a person usually
-- wears; a picture can show other clothes when the story changes them.
ALTER TABLE characters ADD COLUMN outfit TEXT NOT NULL DEFAULT '';
ALTER TABLE personas ADD COLUMN outfit TEXT NOT NULL DEFAULT '';

-- Split the starter tags into looks and outfit, and move the setting words
-- out, because they clash with a changing scene. Only rows that the user did
-- not edit change.
UPDATE characters SET appearance_tags = 'short silver hair, scar on left cheek, grey eyes',
    outfit = 'worn navy flight jacket', art_style = 'sci-fi'
WHERE starter_key = 'mara_voss' AND art_style = ''
    AND appearance_tags = 'short silver hair, scar on left cheek, grey eyes, worn navy flight jacket, sci-fi';
UPDATE characters SET appearance_tags = 'long white beard, round spectacles, ink-stained fingers',
    outfit = 'deep green robes', art_style = 'fantasy'
WHERE starter_key = 'elden_thorne' AND art_style = ''
    AND appearance_tags = 'long white beard, round spectacles, deep green robes, ink-stained fingers, fantasy';
UPDATE characters SET appearance_tags = 'messy black hair, stubble',
    outfit = 'loose tie, long grey trench coat', art_style = 'noir'
WHERE starter_key = 'ren_kagami' AND art_style = ''
    AND appearance_tags = 'messy black hair, stubble, loose tie, long grey trench coat, cigarette smoke, rain, noir';
UPDATE characters SET appearance_tags = 'auburn hair in a messy bun, freckles',
    outfit = 'green apron'
WHERE starter_key = 'june_hale' AND art_style = ''
    AND appearance_tags = 'auburn hair in a messy bun, freckles, green apron, cozy cafe, soft light';
UPDATE characters SET appearance_tags = 'pale skin, black hair',
    outfit = 'long dark coat', art_style = 'gothic'
WHERE starter_key = 'ash' AND art_style = ''
    AND appearance_tags = 'pale skin, long dark coat, black hair, candle light, old manor, gothic';
UPDATE personas SET appearance_tags = 'messy brown hair, stubble',
    outfit = 'brass goggles, worn olive jacket'
WHERE starter_key = 'kai' AND appearance_tags = 'messy brown hair, stubble, brass goggles, worn olive jacket';
UPDATE personas SET appearance_tags = 'wavy brown hair, round glasses',
    outfit = 'mustard scarf, denim jacket, notebook'
WHERE starter_key = 'nora' AND appearance_tags = 'wavy brown hair, round glasses, mustard scarf, denim jacket, notebook';

-- Elden opened almost every reply with his catchphrase, and an example
-- taught him to answer a book request with a riddle. Only an unedited card
-- changes.
UPDATE characters SET example_dialogue = '{{user}}: Where am I?
{{char}}: *He peers over his spectacles.* Somewhere between chapter one and the index, my dear. It is the oldest question in the building, and you are welcome to stay while you think about it.
{{user}}: Can you help me find a book about dragons?
{{char}}: Dragons? Third aisle, top shelf, the red one that is warm to the touch. *He fetches it and hands it over with care.* Read it slowly. It reads you back, you know.',
    voice_anchor = 'Warm and slow. Old-fashioned words ("my dear", "indeed") now and then, never at the start of a reply. Gives what is asked first, then adds a story or a riddle. Often loses his train of thought.'
WHERE starter_key = 'elden_thorne' AND example_dialogue = '{{user}}: Where am I?
{{char}}: *He peers over his spectacles.* Indeed, my dear, that is the oldest question in the building. The short answer is: somewhere between chapter one and the index.
{{user}}: Can you help me find a book about dragons?
{{char}}: Dragons? Oh, they dislike being looked up. But a riddle might coax one out. What has pages but never reads?'
    AND voice_anchor = 'Warm and slow. Old-fashioned words ("indeed", "my dear"). Answers questions with riddles. Often loses his train of thought.';
