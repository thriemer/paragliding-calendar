# travelai

## Commands

**Backend**

Always run the full tests. Dont test part of the code

```
cargo build
cargo test
cargo run
```

**Frontend** (in `frontend/`)
```
npm run dev        # dev server on :3001
npm run build      # typecheck + vite build
npm test           # vitest
npm run typecheck
```

## Code search

Use the **ken MCP server** for codebase searches (`mcp__ken__search`, `mcp__ken__find_related`, etc.). Fall back to grep/Read only when ken doesn't surface what you need.

## Key rules

- Deps are constructor-injected fields on long-lived structs in `AppState`, not function params
- Prefer the smallest change that meets the goal; don't bundle restructuring unless asked
- After every feature, think about if the ARCHITECTURE.md has to be updated, but don't add noise
