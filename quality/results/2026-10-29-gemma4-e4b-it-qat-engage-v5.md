# Quality report: gemma4:e4b-it-qat, 2026-10-29

Prompt version 5. Script "casual". Label "engage-v5". The run took 4 minutes.

Context 8192. Thinking off. GPU share of the model: 100%.

The counts come from heuristics. A count shows where to read the replies, not a final verdict. The full chats are in the replies file next to this one.

This version does not test a chat past the summary point, because summaries come in a later phase.

## Scripted chats

| Character | Style | Replies | Avg words | In range | Recall | Setting openers | Dialogue share | Silent regenerates | Break hints | Breaks left | Repeats | Speaks for user | Narrates user | Guard cut user lines | Name prefix (raw / final) | Persona trait claims | Wrong persona name | Macro left | Avg seconds |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Ren Kagami | conversational | 12 | 58 | 100% | 0 / 0 | 0 | 70% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 1.9 |
| Ren Kagami | texting | 12 | 9 | 100% | 0 / 0 | 0 | 62% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.3 |
| Ren Kagami | storyteller | 12 | 169 | 91% | 0 / 0 | 0 | 45% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 3.6 |
| June Hale | conversational | 12 | 74 | 100% | 0 / 0 | 0 | 64% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 1.7 |
| June Hale | texting | 12 | 16 | 100% | 0 / 0 | 0 | 1% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.5 |
| June Hale | storyteller | 12 | 218 | 58% | 0 / 0 | 0 | 48% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.7 |
| Ash | conversational | 12 | 65 | 100% | 0 / 0 | 0 | 69% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 1.4 |
| Ash | texting | 12 | 14 | 100% | 0 / 0 | 2 | 33% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.4 |
| Ash | storyteller | 12 | 184 | 91% | 0 / 0 | 0 | 52% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 3.7 |
| All |  | 108 | 90 | 93% | 0 / 0 | 2 | 52% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.0 |

In range: the reply length is in the word range of the style. Recall: questions about facts that the user said earlier, answered with the right word. Setting openers: replies whose first sentence is about the weather or the room. Dialogue share: words inside quotes. Silent regenerates: the guard found a problem and the core made the reply again. Break hints: the second reply also broke character, so the user saw a hint. Guard cut user lines: the model wrote for the user and the guard removed it.

## Replies to score by hand

Read each reply and score how well it matches the character's voice, from 1 (generic) to 5 (exactly the character).

### Ren Kagami

- Turn 7 (conversational): *Ren Kagami leans forward slightly, his arms resting on the scarred surface of the desk.* You think I run this place on games. People come here when the light fails them. You need me to pull something out of the mud. What you're hiding is probably bigger than just a flat tire. Tell me the facts, Kai.
  Voice score (1 to 5): __
- Turn 1 (texting): This office. I sell truths and cheap lies. Your world is outside.
  Voice score (1 to 5): __
- Turn 7 (texting): *He lets the smoke drift toward the rain-streaked window.* Show me.
  Voice score (1 to 5): __
- Turn 10 (texting): "You're still listening to the noise."
  Voice score (1 to 5): __
- Turn 9 (storyteller): *Ren stills mid-movement, his grey eyes narrowing as he tilts his head toward the source of the noise. He does not flinch, simply waiting for the sound to repeat. The only sound otherwise is the relentless drizzle against the glass. He shifts his weight in the old chair, the springs groaning in protest beneath him.* It was the building settling. Or perhaps you simply have too much restless energy ...
  Voice score (1 to 5): __

### June Hale

- Turn 7 (conversational): *June playfully bounces on the balls of her feet, her auburn hair catching the soft light filtering through the window.* "Interesting? Well, this old cafe has seen a fair bit! I’ve got some marvelous blueberry scones today that are just out of this world, or maybe we could look out the window at the old railway bridge? It's quite a sight when the sun finally breaks through." *She beams at Kai, til...
  Voice score (1 to 5): __
- Turn 1 (texting): Oh, you're in "The Daily Grind," right here in the city center. Been traveling long?
  Voice score (1 to 5): __
- Turn 7 (texting): Hmm, interesting things? We have a lovely small bookstore next door! Is that something you might like?
  Voice score (1 to 5): __
- Turn 10 (texting): It's just the old building, nothing to worry about! Are you ever near the old train tracks?
  Voice score (1 to 5): __
- Turn 9 (storyteller): *June Hale tilts her head, a small, concerned wrinkle appearing between her eyebrows as she listens intently to Kai.* "Oh? You heard something? What kind of sound were you thinking of?" *She pauses her gentle lean against the counter, her attention completely focused on you, her hands dropping to her sides.* *June takes a quick step toward the table, peering around the edge of the counter toward t...
  Voice score (1 to 5): __

### Ash

- Turn 3 (conversational): *Ash tilts their head slightly, the movement almost imperceptible in the dim light.* It is not a performance, Kai. This is simply how the world is experienced within these walls. The manor speaks in echoes and shadows, and I am merely its most attentive listener. Do you require a fire lit? The hearth has been waiting.
  Voice score (1 to 5): __
- Turn 10 (conversational): *Ash begins to move, their long coat trailing behind them like a shadow, and walks toward the darkened archway.* The library awaits, Kai. It is a place where the dust has collected centuries of whispered knowledge. *Ash pauses at the entrance, a single ray of moonlight catching the edge of a nearby tomes.* The weight of the quiet here can be considerable, I must warn you.
  Voice score (1 to 5): __
- Turn 1 (texting): The house knows its own location. It is a very large dwelling.
  Voice score (1 to 5): __
- Turn 8 (texting): *Ash takes a slow, deliberate step further into the darkness.* The house remembers things.
  Voice score (1 to 5): __
- Turn 3 (storyteller): *Ash slowly turns their gaze back to Kai, the movement smooth and devoid of any hurried intent. They hold the gaze for a beat too long, their expression not shifting in the slightest, as if deciphering a language Kai hasn't even begun to speak.* I am never unlike this, Kai. To understand this house is to understand the necessity of these particular silences. The manor does not offer simple explana...
  Voice score (1 to 5): __
