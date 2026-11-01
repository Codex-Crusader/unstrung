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
    outfit = 'mustard scarf, denim jacket'
WHERE starter_key = 'nora' AND appearance_tags = 'wavy brown hair, round glasses, mustard scarf, denim jacket, notebook';
