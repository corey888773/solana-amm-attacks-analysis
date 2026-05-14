---
name: auggie-explore
description: Use when exploring an unfamiliar repo, feature area, bug path, architecture, or many files. Delegate broad Auggie MCP/codebase-retrieval exploration to a subagent so the main context receives a focused report instead of raw tool dumps. Do not use for tiny exact-file edits or single known-symbol lookups.
---

# Auggie Explore

Goal: broad repo exploration goes through a subagent; main context gets signal,
not raw search noise.

Workflow:
1. If scope spans unknown files, modules, architecture, or a broad bug path,
   spawn a subagent first.
2. Tell the subagent to start with Auggie/codebase-retrieval.
3. After Auggie points to relevant areas, the subagent should read concrete
   files normally, preferably with Serena when available.
4. The subagent may include real detail: important symbols, file paths,
   control flow, data flow, tests, and risks.
5. Avoid raw tool dumps, giant snippets, and "I searched X" narration.
6. Main agent should not repeat the same broad search; use the report, then
   inspect/edit only the narrow files needed.

Subagent prompt:

```text
Explore this repo area. Start with Auggie/codebase-retrieval, then read the
specific files it points to, preferably with Serena. Do not edit files.
Return a focused report: key files/symbols, behavior, data flow, edit points,
risks, and suggested tests. Include enough detail to act, but no raw tool dumps.
Task: <specific question>
```

Use direct local reads only when:
- the relevant file path is already known,
- the lookup is small and exact,
- or the subagent report is back and you are verifying a narrow point.
