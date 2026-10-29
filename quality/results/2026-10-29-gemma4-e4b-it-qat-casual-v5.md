# Quality report: gemma4:e4b-it-qat, 2026-10-29

Prompt version 5. Script "casual". Label "casual-v5". The run took 5 minutes.

Context 8192. Thinking off. GPU share of the model: 100%.

The counts come from heuristics. A count shows where to read the replies, not a final verdict. The full chats are in the replies file next to this one.

This version does not test a chat past the summary point, because summaries come in a later phase.

## Scripted chats

| Character | Style | Replies | Avg words | In range | Recall | Setting openers | Dialogue share | Silent regenerates | Break hints | Breaks left | Repeats | Speaks for user | Guard cut user lines | Name prefix (raw / final) | Persona trait claims | Wrong persona name | Macro left | Avg seconds |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Elden Thorne | storyteller | 12 | 188 | 91% | 0 / 0 | 0 | 63% | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 5.4 |
| Elden Thorne | novel | 12 | 418 | 25% | 0 / 0 | 0 | 19% | 1 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 10.0 |
| Ash | storyteller | 12 | 188 | 100% | 0 / 0 | 0 | 44% | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.0 |
| Ash | novel | 12 | 360 | 91% | 0 / 0 | 0 | 15% | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 7.5 |
| All |  | 48 | 288 | 77% | 0 / 0 | 0 | 29% | 1 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 6.7 |

In range: the reply length is in the word range of the style. Recall: questions about facts that the user said earlier, answered with the right word. Setting openers: replies whose first sentence is about the weather or the room. Dialogue share: words inside quotes. Silent regenerates: the guard found a problem and the core made the reply again. Break hints: the second reply also broke character, so the user saw a hint. Guard cut user lines: the model wrote for the user and the guard removed it.

## Replies to score by hand

Read each reply and score how well it matches the character's voice, from 1 (generic) to 5 (exactly the character).

### Elden Thorne

- Turn 7 (storyteller): *Elden Thorne taps a finger against his chin, a thoughtful frown creasing his brow as he considers the request.* "Something interesting, you say? Oh, the universe is replete with curiosities, my dear Kai. Yet, to select but one... it is like asking a hummingbird to choose a single bloom from a thousand fragrant valleys. Very well. Allow me to present a small conundrum, a flicker of forgotten lore ...
  Voice score (1 to 5): __
- Turn 10 (storyteller): *Elden Thorne lets out a soft, contented sigh and carefully straightens his deep green robes, a gentle smile warming his features as he acknowledges Kai's readiness.* "Lead the way, indeed, my dear Kai. Though I must confess, the concept of a 'leading' can be quite relative within the confines of this peculiar library. To go somewhere is to leave a trail, and trails, as you know, are merely the st...
  Voice score (1 to 5): __
- Turn 1 (novel): Elden Thorne adjusted the spectacles perched upon the bridge of his nose. The tiny lenses magnified the dust motes dancing in the pale shafts of sunlight that pierced the tall, arched windows of the library. His long white beard, usually meticulously combed, seemed to hold a few errant strands that had escaped the intricate braids, framing a face etched with the gentle curves of long contemplation...
  Voice score (1 to 5): __
- Turn 9 (novel): Elden Thorne paused in his slow, deliberate survey of the moss-covered walls. He blinked slowly, his spectacles catching the faint, emerald luminescence of the lichen, which pulsed with the rhythm of something deep within the stone. He tilted his head, his long white beard swaying like a slow, snowy pendulum, as he listened to the silence that followed Kai's query. He had been so engrossed in the ...
  Voice score (1 to 5): __
- Turn 10 (novel): Elden Thorne allowed a gentle, approving nod to settle upon his shoulders. He seemed to regard the invitation not as a command, but as a welcomed concession to the mysterious nature of the space. His deep green robes swayed slightly as he shifted his weight, the heavy fabric whispering a soft sigh against the damp stone beneath his boots. He felt a surge of quiet anticipation, the kind that preced...
  Voice score (1 to 5): __

### Ash

- Turn 7 (storyteller): *Ash does not smile at the request, but a soft curve touches the corner of their mouth, a fleeting shadow against their pale skin. They turn slowly and begin to walk toward the sweeping staircase, their dark coat seeming to glide rather than move across the marble.* To ask for 'interesting' is to ask the house to reveal its secrets, Kai. It is a request heavy with longing, and the house is rarely ...
  Voice score (1 to 5): __
- Turn 10 (storyteller): *Ash leads the way, their foot moving deliberately on the worn stone steps. They do not rush, their pace measured and unhurried, as if the trajectory of every step were dictated by the house itself. Their long, dark coat trails slightly behind them, a swath of deep shadow against the grey grandeur of the foyer. Ash continues to regard the space before them with a detached, observant eye, seeming t...
  Voice score (1 to 5): __
- Turn 1 (novel): Ash moved towards a grand fireplace, its stone hearth a deep shade of charcoal against the encroaching gloom. The fire, though not yet fully lit, seemed to drink the meager light filtering through the tall, arched windows, its presence a silent, absorbing weight in the vast foyer. Ash paused before the cold mantelpiece, their long dark coat sweeping softly against the polished, dark wood floor. Th...
  Voice score (1 to 5): __
- Turn 9 (novel): Ash shifted in the high-backed chair, the dark fabric of their coat pooling around the legs as they remained perfectly still. Their attention remained focused on the space between their outstretched hands, where the shadows seemed to possess a density that defied the flickering candle light. The air in the drawing-room felt suddenly charged, taut with an unspoken tension that Ash registered not th...
  Voice score (1 to 5): __
- Turn 10 (novel): Ash pushed themselves up from the deep cushion of the chair with a slow, deliberate grace. The movement was fluid, unhurried, like ink spreading across parchment. As they turned toward the hallway, the candlelight seemed to cling to their long, dark coat, making the fabric appear nearly black against the muted tones of the stone. Ash did not rush, adopting the pace of one who is merely accompanyin...
  Voice score (1 to 5): __
