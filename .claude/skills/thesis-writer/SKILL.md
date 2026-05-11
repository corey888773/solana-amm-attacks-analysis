---
name: thesis-writer
description: Use when writing or updating a chapter/section of the master's thesis in `thesis/chapters/*.tex`. Triggers on phrases like "napisz rozdział", "pisz thesis", "rozdział 2", "fill in chapter X", "lecimy z pisaniem", "uzupelnij sekcję". Coordinates context gathering (auggie-explore for code, doc-reader for `.docs/`), English LaTeX prose, citation discipline, and writing-status tracking.
---

# Thesis writer

Goal: produce English LaTeX prose for `thesis/chapters/*.tex` from existing `.docs/` artifacts and code, without inventing facts or duplicating effort.

## Hard constraints

- **Language:** English (thesis prose). Comments in `.tex` may stay short.
- **Mode default:** theory + methods only. Do **not** write Ch 7 (Results), Ch 8.1, Ch 8.2 unless user explicitly unblocks.
- **Citations:** every formula, claim from literature, or external fact must cite `bibliography/refs.bib` via `\cite{key}`. No orphan math. If key missing, STOP and ask — do not invent.
- **No fabrication:** if a number, behavior, or formula is not in `.docs/`, code, or a cited paper, do not write it. Ask user.
- **Polish leftovers:** scan section/subsection titles and labels — translate any Polish remnants.
- **TODO comments:** replace `% TODO: …` with prose; if you can't fill, leave a `% TODO: <specific question>` so it's traceable.
- Stay caveman in chat updates; LaTeX prose itself is academic English (no fragments).

## Workflow per chapter

1. **Read status:** `.docs/thesis/writing-status.md` — confirm chapter is in scope and not deferred.
2. **Read scaffold:** current `thesis/chapters/<file>.tex` to see section structure and TODO markers.
3. **Gather context:**
   - For `.docs/`-sourced material (methodology, hypotheses, primers, audits): read directly with `Read`.
   - For code references (e.g., "describe ternary search in `crates/amm-math/...`"): use **auggie-explore subagent** — do not grep manually. Prompt the subagent for symbols, file:line, behavior, edge cases.
   - For literature lookups (find a formula, definition, claim across papers): query `mcp__auggie__codebase-retrieval` with `directory_path=/Users/piotrgasiorek/Studia/magisterka/thesis/papers`. Auggie indexes `.txt` extracts alongside PDFs and returns file:line + quoted text. Use to (a) locate the exact bib key for a formula before citing, (b) verify a claim is in the paper before paraphrasing, (c) discover related results across papers without manual scanning. Do **not** invent equation numbers — quote the snippet auggie returns.
   - For 3rd-party libs/protocols (Raydium, Whirlpool, Jito): Context7 MCP first.
4. **Draft:** fill `% TODO:` blocks with English prose. Keep math in `equation`/`align`. Use `\cite{}` from `refs.bib`. Mirror notation across chapters (Ch 2 sets it).
5. **Self-check:**
   - Every formula cited?
   - Every claim from literature cited?
   - Section title and label English?
   - Notation consistent with Ch 2?
   - No deferred-section content leaked in?
6. **Build check (optional):** if substantial change, suggest `cd thesis && make` to user — do not run unless asked.
7. **Update status:** edit `.docs/thesis/writing-status.md` — flip `[ ]` → `[~]` (partial) or `[x]` (done), bump "Last updated".
8. **Route findings:** if writing surfaced a new gotcha/decision, append to `.docs/notes/findings.md` per doc-writer rules. Do not bury in chapter prose.

## Source mapping (quick lookup)

| Chapter | Primary `.docs/` sources | Code refs |
|---|---|---|
| 1 Introduction | `thesis/research-hypotheses.md`, `thesis/scenarios.md`, `thesis/solana-primer.md` | — |
| 2 AMM math | `thesis/amm-model-justification.md`, `thesis/solana-primer.md` | `crates/amm-math/src/{cpmm,clmm,sandwich}/` |
| 3 SoTA | `bibliography/refs.bib`, Werner+Xu SoK papers (auggie on `papers/` dir) | — |
| 4 Architecture | `reference/guide.md`, `reference/code-reading-roadmap.md`, `research/mainnet-fork.md` | workspace `Cargo.toml`, `fork/`, `crates/` |
| 5 Implementation | audits + code-reading-roadmap | `fork/src/historical_{cpmm,clmm}/`, `crates/amm-math/` |
| 6 Verification | `audits/2026-05-clmm-model-validation.md` (methodology lift), LiteSVM tests | `fork/tests/`, `crates/amm-math/tests/` |
| 8.3 Limitations | `thesis/historical-raydium-methodology.md`, `audits/2026-05-clmm-optimizer-port.md` (caveats sections only) | — |
| 8.4 Future work | `roadmap/tasks.md` | — |

## Order (default)

2 → 4 → 5 → 6 → 3 → 1 → 8.3 → 8.4

Skip 7.x, 8.1, 8.2.

## Failure modes (do not do)

- Reading random files without auggie-explore for code questions.
- Writing prose with no `\cite{}` for non-trivial claims.
- Translating Polish `.docs/` content verbatim into thesis (rephrase to academic English).
- Editing `archive/` or pre-compaction backups.
- Filling deferred sections "while we're at it".
- Leaving Polish labels/titles in scaffold.

## Report-back format

After each chapter (or major section):
- Files touched (path:section).
- Citations added (bib keys).
- Open `% TODO:`s remaining and why.
- Status table line updated.
