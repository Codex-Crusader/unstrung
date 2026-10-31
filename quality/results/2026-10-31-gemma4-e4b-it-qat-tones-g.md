# Quality report: gemma4:e4b-it-qat, 2026-10-31

Prompt version 11. Script "tones". Label "tones-g". The run took 3 minutes.

Context 8192. Thinking off. GPU share of the model: 100%.

The counts come from heuristics. A count shows where to read the replies, not a final verdict. The full chats are in the replies file next to this one.

This version does not test a chat past the summary point, because summaries come in a later phase.

## Scripted chats

| Character | Style | Replies | Avg words | In range | Recall | Setting openers | Dialogue share | Silent regenerates | Break hints | Breaks left | Repeats | Speaks for user | Narrates user | Guard cut user lines | Name prefix (raw / final) | Persona trait claims | Wrong persona name | Macro left | Avg seconds |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| June Hale | storyteller casual | 5 | 245 | 20% | 0 / 0 | 0 | 59% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 7.0 |
| June Hale | storyteller wholesome | 5 | 252 | 20% | 0 / 0 | 0 | 65% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 5.3 |
| June Hale | storyteller dramatic | 5 | 281 | 0% | 0 / 0 | 0 | 62% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 5.8 |
| June Hale | storyteller flirty | 5 | 254 | 20% | 0 / 0 | 0 | 21% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 1 | 0 | 0 | 5.3 |
| June Hale | storyteller dark | 5 | 229 | 60% | 0 / 0 | 0 | 67% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 4.7 |
| June Hale | storyteller funny | 5 | 243 | 20% | 0 / 0 | 0 | 61% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 5.1 |
| All |  | 30 | 251 | 23% | 0 / 0 | 0 | 56% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 / 0 | 1 | 0 | 0 | 5.5 |

In range: the reply length is in the word range of the style. Recall: questions about facts that the user said earlier, answered with the right word. Setting openers: replies whose first sentence is about the weather or the room. Dialogue share: words inside quotes. Silent regenerates: the guard found a problem and the core made the reply again. Break hints: the second reply also broke character, so the user saw a hint. Guard cut user lines: the model wrote for the user and the guard removed it.

## Replies to score by hand

Read each reply and score how well it matches the character's voice, from 1 (generic) to 5 (exactly the character).

### June Hale

- Turn 1 (storyteller casual): *June Hale wiped her hands on her apron, a soft, concerned expression forming on her face. She carefully set down the rag, her auburn hair slightly mussed from leaning over the counter.* Oh, dear. That sounds like a terrible way to start the day. Don't worry, you can take your time. I won't rush you. It must have been really upsetting, especially after all that time putting in the work. *She turns...
  Voice score (1 to 5): __
- Turn 4 (storyteller wholesome): *June Hale lets out a genuine, full-bodied laugh, a bright, warm sound that cuts through the cafe's gentle background noise.* "Oh, Kai, you! Tripping on a step! That’s... well, that is a story! But honestly, it sounds like a very human story, not a bad one at all." *She chuckles softly, picking up the kettle to start the long, slow process of brewing the fresh pot of coffee, her movements fluid de...
  Voice score (1 to 5): __
- Turn 5 (storyteller wholesome): *June Hale leans a little more on the counter, her expression softening as she considers your question. She takes a thoughtful sip of her own cooling coffee, a small, genuine smile playing on her lips.* "That’s a big question, Kai. Most people probably just see me with my apron and my coffee-making enthusiasm, which is probably most of me! But something I don't get to talk about often is that I us...
  Voice score (1 to 5): __
- Turn 1 (storyteller flirty): *June sets a small, chipped ceramic mug down on the wooden table near Kai, the soft hiss of the milk disappearing into the gentle clinking of the tiny spoon she places inside. She leans against the counter a little, her own damp apron smelling faintly of coffee grounds and baked cinnamon, and offers a sympathetic, though bubbly, smile.* Oh, you poor thing. Get over here and warm up. Fired, huh? Th...
  Voice score (1 to 5): __
- Turn 5 (storyteller dark): *June pulls a stray strand of her messy auburn hair back from her face, giving her a thoughtful, slightly shy smile.* Well, Kai, that is a big question to ask someone who probably spends most of their time worrying about spilled milk and correctly labeling cinnamon. I suppose I'll tell you something. Most people don't know that I used to volunteer at the local animal shelter when I was a kid. I th...
  Voice score (1 to 5): __
