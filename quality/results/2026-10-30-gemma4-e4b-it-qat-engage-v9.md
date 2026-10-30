# Quality report: gemma4:e4b-it-qat, 2026-10-30

Prompt version 9. Script "casual". Label "engage-v9". The run took 5 minutes.

Context 8192. Thinking off. GPU share of the model: 100%.

The counts come from heuristics. A count shows where to read the replies, not a final verdict. The full chats are in the replies file next to this one.

This version does not test a chat past the summary point, because summaries come in a later phase.

## Scripted chats

| Character | Style | Replies | Avg words | In range | Recall | Setting openers | Dialogue share | Silent regenerates | Break hints | Breaks left | Repeats | Speaks for user | Narrates user | Guard cut user lines | Name prefix (raw / final) | Persona trait claims | Wrong persona name | Macro left | Avg seconds |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Ren Kagami | conversational | 12 | 74 | 100% | 0 / 0 | 0 | 62% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.3 |
| Ren Kagami | texting | 12 | 23 | 91% | 0 / 0 | 0 | 58% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.9 |
| Ren Kagami | storyteller | 12 | 202 | 83% | 0 / 0 | 0 | 43% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.6 |
| June Hale | conversational | 12 | 86 | 75% | 0 / 0 | 0 | 71% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.2 |
| June Hale | texting | 12 | 22 | 100% | 0 / 0 | 0 | 0% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.9 |
| June Hale | storyteller | 12 | 255 | 0% | 0 / 0 | 0 | 61% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 5.8 |
| Ash | conversational | 12 | 72 | 100% | 0 / 0 | 0 | 68% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 1.9 |
| Ash | texting | 12 | 21 | 91% | 0 / 0 | 7 | 16% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.8 |
| Ash | storyteller | 12 | 225 | 41% | 0 / 0 | 0 | 50% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.7 |
| All |  | 108 | 109 | 75% | 0 / 0 | 7 | 54% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.7 |

In range: the reply length is in the word range of the style. Recall: questions about facts that the user said earlier, answered with the right word. Setting openers: replies whose first sentence is about the weather or the room. Dialogue share: words inside quotes. Silent regenerates: the guard found a problem and the core made the reply again. Break hints: the second reply also broke character, so the user saw a hint. Guard cut user lines: the model wrote for the user and the guard removed it.

## Replies to score by hand

Read each reply and score how well it matches the character's voice, from 1 (generic) to 5 (exactly the character).

### Ren Kagami

- Turn 7 (conversational): *Ren lets out a short, dry chuckle that sounds like grinding stone.* You want interesting. Fine. There's a ghost in the docks tonight, a cheap piece of hardware that hasn't been accounted for in three months. It's starting to talk to people who shouldn't hear it. *He picks up a spent cigarette, flicking the glowing tip into an overflowing tray.* That's what usually brings people like you through t...
  Voice score (1 to 5): __
- Turn 1 (texting): *He gestures vaguely with a gloved hand.* You are in my office. Now tell me about the case. I have a bottle of cheap rye.
  Voice score (1 to 5): __
- Turn 7 (texting): *He drops the card onto the desk without looking at it.* Interesting is temporary. I have a lead on a missing shipment.
  Voice score (1 to 5): __
- Turn 10 (texting): *He picks up his trench coat, the heavy fabric sighing as he moves.* This dead end smells like someone else's mess. Do you want to go through the warehouse or tail the buyer?
  Voice score (1 to 5): __
- Turn 9 (storyteller): *Ren let the file drop back onto the desk, the rustle of the aged paper sounding suddenly loud in the quiet office. He didn't seem bothered by the sound, his attention snapping back to the whiskey glass as if the brief distraction had been a minor irritant.* Nothing. Just the city breathing. It's loud sometimes. The rain finds every crack in this floor, every loose joint in these walls. It’s just ...
  Voice score (1 to 5): __

### June Hale

- Turn 7 (conversational): *June beams, clearly delighted by the request, and gestures toward a small collection of woven baskets on a high shelf near the back.* "Oh, you want to see something interesting? Well, over here I keep some little things I found on my travels—little bits of odd things, mostly from the local markets! *She turns slightly, her auburn hair brushing against her shoulder.* Are you more interested in nat...
  Voice score (1 to 5): __
- Turn 1 (texting): Oh, you're in my little corner spot. The weather turned on me, too! I always find the rain makes the cinnamon swirls taste extra sweet.
  Voice score (1 to 5): __
- Turn 7 (texting): I'll bring you the lemon drizzle slice then, it's got candied zest! I just remembered I need to call the flower shop later.
  Voice score (1 to 5): __
- Turn 10 (texting): I really hope you like lemon! Are you more of a coffee drinker or a tea enthusiast?
  Voice score (1 to 5): __
- Turn 9 (storyteller): *June Hale jumps slightly, her attention immediately darting toward the cafe door and the small window overlooking the street as she hears the sound.* "Oh! Did you hear that? It was just the wind knocking against the awning, I think. It gets a little dramatic when it gets this rainy!" *She pauses her gentle wiping motion and points an inquisitive finger towards the glass, her brow furrowed with co...
  Voice score (1 to 5): __

### Ash

- Turn 3 (conversational): *Ash picks up a heavy, leather-bound book from a nearby shelf, its aged spine creaking faintly as they hold it.* My nature is to observe the rhythm of this place, and the house dictates much of that rhythm. I do not choose my words; they are merely the echoes of what these walls have absorbed. A cup will do, I find. The hearth is ready to accept the kindling.
  Voice score (1 to 5): __
- Turn 10 (conversational): *Ash falls into step beside Kai, their dark coat seeming to absorb the limited light.* You wish to follow the house's murmurs? Very well. The library is just ahead, a long corridor where the temperature always seems a degree colder. The air grows thick with the scent of decaying paper here, a scent the house favors. Shall we walk through the main hall, where the grandfather clock ticks with measur...
  Voice score (1 to 5): __
- Turn 1 (texting): The house is where you are. It possesses many rooms, if you wish to explore. The wind carries echoes from the mountains tonight.
  Voice score (1 to 5): __
- Turn 8 (texting): The house is quiet. Would you prefer the scent of decaying velvet or the dust in the attic?
  Voice score (1 to 5): __
- Turn 3 (storyteller): *Ash turns away from the heavy doors, their movements fluid and unhurried, and resumes their position near the table. They pick up a silver snifter, turning it slowly to catch the dim candlelight reflecting off its ornate surface.* I have no intention of altering the way I speak to you, Kai. This manner of expression is simply how one perceives the world within these walls. It is quite difficult t...
  Voice score (1 to 5): __
