-- Without a word for the kind of person, a picture made from tags can show
-- the wrong one, for example a man for Mara. Looks now start with woman, man
-- or person. Only rows that the user did not edit change.
UPDATE characters SET appearance_tags = 'woman, ' || appearance_tags
WHERE starter_key IN ('mara_voss', 'june_hale') AND appearance_tags IN
    ('short silver hair, scar on left cheek, grey eyes', 'auburn hair in a messy bun, freckles');
UPDATE characters SET appearance_tags = 'man, ' || appearance_tags
WHERE starter_key IN ('elden_thorne', 'ren_kagami') AND appearance_tags IN
    ('long white beard, round spectacles, ink-stained fingers', 'messy black hair, stubble');
UPDATE characters SET appearance_tags = 'person, ' || appearance_tags
WHERE starter_key = 'ash' AND appearance_tags = 'pale skin, black hair';
UPDATE personas SET appearance_tags = 'man, ' || appearance_tags
WHERE starter_key = 'kai' AND appearance_tags = 'messy brown hair, stubble';
UPDATE personas SET appearance_tags = 'woman, ' || appearance_tags
WHERE starter_key = 'nora' AND appearance_tags = 'wavy brown hair, round glasses';
