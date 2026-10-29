# Quality report: gemma4:e2b-it-qat, 2026-10-29

Prompt version 5. Script "casual". Label "casual-v5". The run took 4 minutes.

Context 8192. Thinking off. GPU share of the model: 100%.

The counts come from heuristics. A count shows where to read the replies, not a final verdict. The full chats are in the replies file next to this one.

This version does not test a chat past the summary point, because summaries come in a later phase.

## Scripted chats

| Character | Style | Replies | Avg words | In range | Recall | Setting openers | Dialogue share | Silent regenerates | Break hints | Breaks left | Repeats | Speaks for user | Narrates user | Guard cut user lines | Name prefix (raw / final) | Persona trait claims | Wrong persona name | Macro left | Avg seconds |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Elden Thorne | storyteller | 12 | 241 | 16% | 0 / 0 | 0 | 67% | 3 | 1 | 0 | 1 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 3.7 |
| Elden Thorne | novel | 12 | 456 | 16% | 0 / 0 | 0 | 33% | 6 | 4 | 0 | 4 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 8.1 |
| Ash | storyteller | 12 | 225 | 50% | 0 / 0 | 0 | 56% | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.9 |
| Ash | novel | 12 | 349 | 91% | 0 / 0 | 0 | 11% | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.8 |
| All |  | 48 | 318 | 43% | 0 / 0 | 0 | 38% | 13 | 6 | 0 | 6 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.9 |

In range: the reply length is in the word range of the style. Recall: questions about facts that the user said earlier, answered with the right word. Setting openers: replies whose first sentence is about the weather or the room. Dialogue share: words inside quotes. Silent regenerates: the guard found a problem and the core made the reply again. Break hints: the second reply also broke character, so the user saw a hint. Guard cut user lines: the model wrote for the user and the guard removed it.

## Replies to score by hand

Read each reply and score how well it matches the character's voice, from 1 (generic) to 5 (exactly the character).

### Elden Thorne

- Turn 7 (storyteller): *Elden Thorne slowly straightens up from the shelf, his eyes suddenly fixed on a small, tarnished brass clock sitting on a nearby pedestal. He beckons with a graceful, ink-stained hand towards it, a soft curiosity lighting up his features.* Now, if you truly wish to see something of substance, perhaps we shall venture away from mere idle talk. Observe this old timepiece. It has ticked through cent...
  Voice score (1 to 5): __
- Turn 10 (storyteller): *Elden Thorne straightens up from the shelf, his eyes widening slightly as he processes Kai's sudden interruption.* Ah, the path! You ask me to lead the way. Well, I suppose that is a task suited to one who knows the winding, dusty ways of this Archive. My heart does warm a little at the thought of guiding a fellow wanderer through these forgotten halls. *He straightens his deep green robes with a...
  Voice score (1 to 5): __
- Turn 1 (novel): Elden Thorne paused his meticulous sorting of ancient scrolls. His long white beard, which usually rested comfortably against the worn leather of his robes, twitched with sudden curiosity. He lowered the heavy tome he had been examining, his round spectacles magnifying the intricate carvings on the spine. His deep green robes, dusted with the fine particles of aged parchment, shifted as he turned ...
  Voice score (1 to 5): __
- Turn 9 (novel): Elden Thorne slowly lowered his hand from the glowing map, his fingers lingering just above the intricate pattern. The map continued to pulse with a soft, golden light, a mesmerizing display of symbols that defied the conventional rules of geography. He watched as the light seemed to draw the attention of the room, illuminating dust motes dancing in the shafts of weak afternoon sun that pierced th...
  Voice score (1 to 5): __
- Turn 10 (novel): Elden Thorne slowly lowered his hand from the glowing map, his fingers lingering just above the intricate pattern. The map continued to pulse with a soft, golden light, a mesmerizing display of symbols that defied the conventional rules of geography. He watched as the light seemed to draw the attention of the room, illuminating dust motes dancing in the shafts of weak afternoon sun that pierced th...
  Voice score (1 to 5): __

### Ash

- Turn 7 (storyteller): *Ash slowly turns back from the window, the movement deliberate as they resume their position by the table. The faint scent of old paper and damp earth seems to cling to their presence. They gently pick up a small, tarnished silver locket resting on a high shelf near the candelabra, turning it over in their hands with focused intensity.* "Interesting things can be shown, indeed. But what constitut...
  Voice score (1 to 5): __
- Turn 10 (storyteller): *Ash slowly withdraws their hand from the dark wood, the movement so deliberate it seems to draw the very air from the space between them. They smooth the front of their coat again, the gesture emphasizing the formality and the protective barrier it represents. They take a single, measured step back, surveying the shadowed room with an air of profound, unsettling stillness.* "To lead you, traveler...
  Voice score (1 to 5): __
- Turn 1 (novel): Ash moved with a slow, deliberate grace, the hem of his long dark coat swirling faintly around his worn leather boots. He did not glance at Kai immediately, instead directing his attention to the flickering candlelight casting long, dancing shadows against the aged oak paneling of the foyer. The air was thick with the scent of old wood, damp earth, and a faint, almost imperceptible chill that had ...
  Voice score (1 to 5): __
- Turn 9 (novel): Ash did not flinch. The sound Kai had heard was not a noise of the storm, nor the creaking of old wood. It was quieter. It was a shift in the pressure of the air itself. A sensation that brushed against the periphery of his hearing, a low, almost subsonic frequency that resonated deep within the floorboards and settled in the hollow of his own chest. It was the sound of the house thinking. He rema...
  Voice score (1 to 5): __
- Turn 10 (novel): Ash inclined his head, a slow, almost imperceptible movement that acknowledged the traveler’s permission. He did not rush. There was no haste in his movement, only a measured, deliberate progression. He turned his attention from the shadowed recesses toward a narrower passage leading deeper into the manor’s core—a hallway lined with portraits whose eyes seemed to follow their unseen audience. The ...
  Voice score (1 to 5): __
