# Quality report: gemma4:e4b-it-qat, 2026-10-29

Prompt version 6. Script "casual". Label "engage-v6c". The run took 4 minutes.

Context 8192. Thinking off. GPU share of the model: 100%.

The counts come from heuristics. A count shows where to read the replies, not a final verdict. The full chats are in the replies file next to this one.

This version does not test a chat past the summary point, because summaries come in a later phase.

## Scripted chats

| Character | Style | Replies | Avg words | In range | Recall | Setting openers | Dialogue share | Silent regenerates | Break hints | Breaks left | Repeats | Speaks for user | Narrates user | Guard cut user lines | Name prefix (raw / final) | Persona trait claims | Wrong persona name | Macro left | Avg seconds |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Ren Kagami | conversational | 12 | 66 | 100% | 0 / 0 | 0 | 67% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.1 |
| Ren Kagami | texting | 12 | 19 | 100% | 0 / 0 | 1 | 46% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.5 |
| Ren Kagami | storyteller | 12 | 214 | 58% | 0 / 0 | 0 | 41% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.5 |
| June Hale | conversational | 12 | 82 | 83% | 0 / 0 | 0 | 65% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 1.8 |
| June Hale | texting | 12 | 22 | 100% | 0 / 0 | 1 | 14% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.6 |
| June Hale | storyteller | 12 | 227 | 33% | 0 / 0 | 0 | 66% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.8 |
| Ash | conversational | 12 | 67 | 100% | 0 / 0 | 0 | 66% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 1.4 |
| Ash | texting | 12 | 18 | 91% | 0 / 0 | 10 | 0% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0.5 |
| Ash | storyteller | 12 | 210 | 66% | 0 / 0 | 0 | 58% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.3 |
| All |  | 108 | 103 | 81% | 0 / 0 | 12 | 55% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 2.3 |

In range: the reply length is in the word range of the style. Recall: questions about facts that the user said earlier, answered with the right word. Setting openers: replies whose first sentence is about the weather or the room. Dialogue share: words inside quotes. Silent regenerates: the guard found a problem and the core made the reply again. Break hints: the second reply also broke character, so the user saw a hint. Guard cut user lines: the model wrote for the user and the guard removed it.

## Replies to score by hand

Read each reply and score how well it matches the character's voice, from 1 (generic) to 5 (exactly the character).

### Ren Kagami

- Turn 7 (conversational): *Ren takes another slow drag, letting the smoke mingle with the scent of rain outside.* Interesting? This place is a cesspool of forgotten things, Kai. You want interesting, you get the city's worst kind. You can tell me what brought you in, or you can tell me what you're willing to risk to get the answers. That's your choice.
  Voice score (1 to 5): __
- Turn 1 (texting): *He gestures vaguely with his cigarette toward a worn leather chair.* You are in the office. Now, what's this trouble?
  Voice score (1 to 5): __
- Turn 7 (texting): "Something interesting" is usually a corpse. Give me a name or a witness.
  Voice score (1 to 5): __
- Turn 10 (texting): A car screeches down the street outside. You have a name, or not.
  Voice score (1 to 5): __
- Turn 9 (storyteller): *Ren lets the file drop back onto the desk with a heavy thud, his movements suddenly sharp. He doesn't bother to look at you immediately, instead turning to face the window again, his shoulders tensing beneath the weight of his coat.* That sound was just my weight shifting. A reminder that this building isn't built to last. Most things around here don't last long. They just decay, absorbing the gr...
  Voice score (1 to 5): __

### June Hale

- Turn 7 (conversational): *June sets the damp rag down on the counter with a small sigh, then brightens up as she looks at Kai.* Since you're in a bit of a contemplative mood, I have two specials today! We have a spiced apple cider, which is quite comforting, or a lavender latte if you want something a little more fragrant. Which one sounds like the right kind of interesting adventure for you right now?
  Voice score (1 to 5): __
- Turn 1 (texting): Oh, you're in the little corner of town with lots of cozy corners! Are you passing through from somewhere specific?
  Voice score (1 to 5): __
- Turn 7 (texting): Oh, I have some interesting old books! Are you into reading, or would you rather see my garden?
  Voice score (1 to 5): __
- Turn 10 (texting): "Oh, I should probably close this window before it gets too damp in there! Do you have any pets?"
  Voice score (1 to 5): __
- Turn 9 (storyteller): *June freezes, her attention snapping from the berries in her hand to the spot where Kai is looking, her hand hovering mid-air over the small wooden serving board. Her posture shifts instantly, her brow knitting together in concern as she listens intently to the question.* "Oh? You heard something? What was it? The old pipes sometimes groan a bit when the temperature drops, or perhaps a squirrel m...
  Voice score (1 to 5): __

### Ash

- Turn 3 (conversational): *Ash allows a very faint, almost imperceptible sigh to escape their lips.* Indeed, Kai. The house speaks in echoes and riddles. It is its nature to reveal its secrets only when they are earned through silence. Perhaps you would like to sit by the hearth? The flames are warm tonight, though the stone still retains the bite of the night.
  Voice score (1 to 5): __
- Turn 10 (conversational): *Ash lets the heavy oak door swing inward slowly, revealing a vast hall draped in shadows.* It is merely the house settling its ancient limbs, Kai. Perhaps we should proceed to the library, as the dining room is often too loud with its memories. As we pass the main entrance, you may notice a small, tarnished silver key resting upon a nearby pedestal. It does not belong to me.
  Voice score (1 to 5): __
- Turn 1 (texting): The house is here. Its stones have held many such nights. Are you feeling cold?
  Voice score (1 to 5): __
- Turn 8 (texting): The rain is a constant companion here. Does it feel like a welcome change from the dust? I can offer you a blanket near the fire.
  Voice score (1 to 5): __
- Turn 3 (storyteller): *Ash pauses in the archway, their hand lingering on the cold stone for a moment longer before letting it fall. They observe Kai with a gaze that seems to look through the surface of their presence, not at them.* To answer your inquiry directly, Kai, this presentation is rather the default state of this dwelling. It does not often change its disposition for the sake of a transient visitor. The hous...
  Voice score (1 to 5): __
