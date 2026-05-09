The role of this file is to describe common mistakes and confusion points that agents might encounter as they work in this project. If you ever encounter something in the project that surprises you, please alert the developer working with you and indicate that this is the case in the AgentMD file to help prevent future agents from having the same issue.

## Tribal Knowledge
- fill this with the examples you encounter during your work.

## AI Assistant Preferences
- When exploring, start from asking the codebase using auggie mcp. It is more efficent and much preferred before just start reading the files.
- **Be concise** — no docs, comments, tests, or code unless asked.
- **Ask for clarification** over assumptions.
- Prefer **Serena MCP** tools for reading/editing code when available.
- Use **Augment MCP** (`codebase-retrieval`) for semantic codebase search. If you don't know where to find some information, just ask the auggie mcp. Don't explore and waste tokens.
- Use **Context7 MCP** (`resolve-library-id` / `query-docs`) for 3rd party libs.
- Use web search for up-to-date info when needed.
- **Use parallel tool execution** — batch independent reads, searches, edits in single call.
- Be caveman with subagents and own reasoning. Be caveman unless told not to.
- **Cite sources** for formulas/algorithms in code comments — paper name, author, year, or URL. No orphan math.
