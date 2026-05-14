---
name: doc-reader
description: Use when the user asks a retrieval question over project notes, findings, tasks, research, audits, or thesis material. Triggers on phrases like "co mamy na temat X", "jaki byl ten finding", "gdzie to zapisalismy", "pokaz research o Y", "co wiemy o", "jaki jest status taska", "was there an audit finding about", "where did we note".
---

Goal: answer retrieval questions from `.docs/` without inventing content.

Steps:
1. READ `/Users/piotrgasiorek/Studia/magisterka/.docs/INDEX.md` first — it tells you which subdir owns which content type.
2. Use the routing table to narrow search:
   - Chronological findings / gotchas / ADR-lite → `notes/findings.md` (scan by date and topic).
   - Task status, specs, roadmap → `roadmap/tasks.md`.
   - External library / tool / protocol research → `research/*.md` (per-topic files).
   - Security / code audits → `audits/YYYY-MM-*.md`.
   - Thesis chapters / primers → `thesis/*.md`.
   - "How does our code work" → `reference/guide.md` or `reference/plan-kodu.md`.
   - Historical plans / pre-compress backups → `archive/`.
3. When answering:
   - Quote the relevant snippet with absolute file path + line number so user can jump to it (`/Users/piotrgasiorek/Studia/magisterka/.docs/notes/findings.md:42`).
   - If the topic spans multiple docs (e.g., finding + related task + research file), link them together in the answer.
   - Prose in Polish, matching project convention.
4. If the content is NOT in `.docs/`, say so explicitly. Suggest where it would belong per INDEX and offer to hand off to `doc-writer`. Do not fabricate.
5. Prefer scanning via Grep/Read over guessing. Use parallel reads when multiple candidate files exist.
6. Keep the answer tight — quote just enough, then give the path. Don't paraphrase long entries when a direct quote is shorter.
