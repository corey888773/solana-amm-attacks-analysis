---
name: doc-writer
description: Use when the user wants to save a finding, write a task spec, document research, record an audit, or add anything to the project's .docs/ directory. Triggers on phrases like "zapisz to gdzies", "zanotuj", "dodaj do notatek", "spisz research", "zapisz finding", "dodaj task", "udokumentuj", "save this finding", "record this", "add to notes".
---

Goal: keep `.docs/` organized per INDEX routing so future sessions don't make a mess.

Steps:
1. READ `/Users/piotrgasiorek/Studia/magisterka/.docs/INDEX.md` first — it defines the routing table and current state. Do not skip.
2. Classify the content and pick target:
   - Finding / gotcha / observation / ADR-lite → append to `notes/findings.md` using the template at the bottom of that file (sections: Źródło, Kontekst, Finding, Implikacje, TODO w pracy, Implementacja). New entry header: `## YYYY-MM-DD — <tytuł>`.
   - Task / feature spec / forward work → append `## Task X — <title>` to `roadmap/tasks.md` with Status, Motywacja, Goal, Approach, Deliverables, Acceptance criteria, Non-goals.
   - Research on a library/tool/protocol → NEW file `research/<kebab-case>.md` (don't dump into existing research files).
   - Audit / security review → `audits/YYYY-MM-<scope>.md`, severity-prioritized list.
   - Thesis-bound prose → `thesis/<topic>.md`.
   - Internal how-it-works guide → `reference/guide.md` (narrative).
3. Conventions (from INDEX + project AGENTS.md):
   - Prose in Polish, code/configs/commits/technical terms in English.
   - ISO dates `YYYY-MM-DD`. Absolute, never relative ("wczoraj", "ostatnio").
   - Cite sources for math formulas: paper, author, year, section/equation.
   - Kebab-case research filenames; audit filename pattern `YYYY-MM-<scope>.md`.
   - Caveman prose — tight, no fluff, no unnecessary prose.
4. Never write outside `.docs/` unless user explicitly asks (e.g., README, AGENTS.md).
5. Never edit `archive/` — read-only.
6. After writing, update `.docs/INDEX.md` "Aktualna zawartość" section if you created a new file or materially changed the state summary of an existing one.
7. If the bucket is unclear (e.g., could be finding or research), ASK the user — don't guess.
8. Report back the exact file path(s) touched and one-line summary of what was added.
