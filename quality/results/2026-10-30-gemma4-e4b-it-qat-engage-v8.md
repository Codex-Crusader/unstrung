# Quality report: gemma4:e4b-it-qat, 2026-10-30

Prompt version 7. Script "casual". Label "engage-v8". The run took 5 minutes.

Context 8192. Thinking off. GPU share of the model: 100%.

The counts come from heuristics. A count shows where to read the replies, not a final verdict. The full chats are in the replies file next to this one.

This version does not test a chat past the summary point, because summaries come in a later phase.

## Scripted chats

| Character | Style | Replies | Avg words | In range | Recall | Setting openers | Dialogue share | Silent regenerates | Break hints | Breaks left | Repeats | Speaks for user | Narrates user | Guard cut user lines | Name prefix (raw / final) | Persona trait claims | Wrong persona name | Macro left | Avg seconds |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Ren Kagami | conversational | 12 | 72 | 100% | 0 / 0 | 0 | 73% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.4 |
| Ren Kagami | texting | 12 | 21 | 91% | 0 / 0 | 0 | 57% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.9 |
| Ren Kagami | storyteller | 12 | 201 | 66% | 0 / 0 | 0 | 36% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.5 |
| June Hale | conversational | 12 | 78 | 100% | 0 / 0 | 0 | 66% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.2 |
| June Hale | texting | 12 | 22 | 100% | 0 / 0 | 2 | 1% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 1.0 |
| June Hale | storyteller | 12 | 245 | 25% | 0 / 0 | 0 | 39% | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 1 | 0 | 0 | 6.0 |
| Ash | conversational | 12 | 75 | 100% | 0 / 0 | 0 | 60% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.0 |
| Ash | texting | 12 | 17 | 100% | 0 / 0 | 5 | 0% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.8 |
| Ash | storyteller | 12 | 230 | 41% | 0 / 0 | 0 | 36% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.9 |
| All |  | 108 | 107 | 80% | 0 / 0 | 7 | 43% | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 1 | 0 | 0 | 2.7 |

In range: the reply length is in the word range of the style. Recall: questions about facts that the user said earlier, answered with the right word. Setting openers: replies whose first sentence is about the weather or the room. Dialogue share: words inside quotes. Silent regenerates: the guard found a problem and the core made the reply again. Break hints: the second reply also broke character, so the user saw a hint. Guard cut user lines: the model wrote for the user and the guard removed it.

## Replies to score by hand

Read each reply and score how well it matches the character's voice, from 1 (generic) to 5 (exactly the character).

### Ren Kagami

- Turn 7 (conversational): *Ren takes a long, slow drag from his cigarette before letting the smoke coil around his head.* You want something interesting. The only interesting thing I deal with is the part the official report forgot. I push the stack of case files aside, revealing a folder stamped 'Cold'. This city runs on secrets, Kai, and that's the only currency I trade in. Now, are you going to tell me why you're here, ...
  Voice score (1 to 5): __
- Turn 1 (texting): *Ren lets out a slow plume of smoke into the stagnant air.* You're in my office. You look like you've seen more rain than sense.
  Voice score (1 to 5): __
- Turn 7 (texting): *Ren stubs out the cigarette, leaving a smear of ash on the floor.* Show you anything is a luxury you can't afford. Tell me why you're here.
  Voice score (1 to 5): __
- Turn 10 (texting): *Ren stares at you, a flicker of something near amusement crossing his eyes.* 'Lead the way' means you start. I'm waiting for the details, kid.
  Voice score (1 to 5): __
- Turn 9 (storyteller): *Ren slowly drags the cigarette from his mouth, the ember glowing a harsh orange in the dim light. He flicks the glowing tip out of the ashtray with a flick of his wrist, then lets the ash fall into the ceramic. He takes a shallow breath, the air tasting of stale smoke and damp pavement.* The sound is just the city exhaling. Not worth the attention. This office hears a lot more things than it lets...
  Voice score (1 to 5): __

### June Hale

- Turn 7 (conversational): *June brightens and gestures toward a small, slightly overflowing display case tucked to the side of the counter, filled with baked goods.* "Ooh, well, since you asked! I just got this new batch of spiced pear fritters. They have these little crystallized ginger bits inside, and they're just perfect with a cup of tea. *She beams at Kai, adjusting the display slightly.* Tell me, what kind of spices...
  Voice score (1 to 5): __
- Turn 1 (texting): Oh, you're in "The Daily Grind," right on Elm Street! Are you heading somewhere nice after this rain?
  Voice score (1 to 5): __
- Turn 7 (texting): I just learned about a new supplier for my beans! We have fresh blueberry scones today, if you fancy one.
  Voice score (1 to 5): __
- Turn 10 (texting): Oh, you're ready for action! I don't know how to lead anyone, I'm quite the scatterbrained mess, though. I just finished wiping down the counter.
  Voice score (1 to 5): __
- Turn 9 (storyteller): *June Hale lets out a soft, airy laugh, a warm, quick sound, and turns back to the counter, her auburn hair shifting over her shoulder as she taps a finger against her green apron.* Oh, that? *She raises an eyebrow playfully.* That was just the digital ghost of my online orders, Kai! *She turns back to him, her expression softening instantly.* I know it can be a bit jarring when a cozy little spot...
  Voice score (1 to 5): __

### Ash

- Turn 3 (conversational): *Ash tilts their head slightly, the candlelight catching the severity of their expression.* My nature is to simply be, Kai; the house merely reflects what is around it. My responses are rarely simple, much like the winding paths within these grounds. I offer what is necessary for the present moment. *Ash moves toward a nearby shadowed sitting area and carefully arranges a small, unlit candle upon ...
  Voice score (1 to 5): __
- Turn 10 (conversational): Ash slowly rises from the armchair, their dark silhouette seeming to grow longer in the meager candlelight. "To lead you is to invite you deeper into my surroundings, Kai, which is a decision you make entirely of your own volition." *Ash turns and begins to walk toward a narrow, dark archway leading to the manor's main corridor, their steps soft on the thick, dusty floorboards.* The house finds mo...
  Voice score (1 to 5): __
- Turn 1 (texting): The house does not boast a map. It merely knows that you are within its shadow. You may rest by the hearth.
  Voice score (1 to 5): __
- Turn 8 (texting): The library is vast. A book fell from its shelf just now.
  Voice score (1 to 5): __
- Turn 3 (storyteller): *Ash gently closes the old volume, the sound of the leather making a soft, dry rasp in the quiet room. They place the book down with meticulous care, ensuring the cover remains perfectly aligned with the cushion of the chair.* My demeanor is rarely different when I am within these walls, Kai. The house is a demanding host, and I merely exist within the parameters of its temperament. It is a deeply...
  Voice score (1 to 5): __
