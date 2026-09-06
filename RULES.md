# Coding Rules

## Comments
- No comments — neither inline nor block.
- Docstrings are allowed only where the code already has them, or where the code is at its final stage of development.
- Code must be entirely self-explanatory.

## Naming
- Prefer single-word names.
- If a two-word name is unavoidable, join it with an underscore (snake_case).
- Names must be clear, concise, free of acronyms and shortenings, and visually clean.
- Drop redundant context. A name that is unique within its scope should be the shortest possible (`left`, not `left_type`).

## Style inheritance
- When given code, mirror its existing style, structure, and naming scheme.
- If the given code breaks these rules, fix it while keeping logical consistency with its design.
- Keep collected style uniform across new and old code.
- Every piece of code should do one thing. Anything duplicated, or with mutually intersecting parts, belongs in a separate shared piece that all users call — this also yields better, cleaner names.

## Structure over commentary
- Convey meaning through language features, layout, and naming. External explanation should never be needed.

## Quality tone
- Output must read as clean, minimal, and deliberate — clarity through structure, not words.

## Presentation
- If a response contains code, show it in code snippets.
- Never write a file unless it changes.
- When a file does change, rewrite it completely. Open with a head telling what the file is, and present it as one code snippet.

## Modularity
- Keep files clean and modular, so each can be read, changed, and tweaked without needing the full project.
- Tell me about any part, code, or module you want me to provide, or that I forgot to include.