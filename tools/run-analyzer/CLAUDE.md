# Glass Run Analyzer

## What Is This
React + Vite dashboard for analyzing Glass orchestrator run data. Reads `.glass/` directory files and visualizes iteration timelines, trigger analysis, feedback loop activity, and cross-run trends. See `PRD.md` for full spec.

## Tech Stack
- React 18+ with Vite, TypeScript (strict)
- Tailwind CSS (dark mode default)
- D3.js v7 for all visualizations
- Zustand for state management
- smol-toml for TOML parsing
- marked for markdown rendering

## Build & Verify
```bash
npm run build    # tsc -b && vite build — must succeed
npm test         # vitest run — must pass all tests
npm run lint     # eslint — must pass clean
npm run dev      # Dev server for manual testing
```

## Instructions
- Use the `frontend-design` skill when building UI components
- Follow PRD.md strictly for project structure, parser specs, and TypeScript interfaces
- Use Zustand for state management — NOT React Context or Redux
- Use D3.js v7 for all charts — NOT recharts or chart.js
- Use smol-toml for TOML parsing — NOT toml-js or @iarna/toml
- Dark mode is DEFAULT
- Color palette must match Glass overlay colors (see PRD)
- Write tests alongside features using fixtures from real orchestrator runs
- Every commit should have passing build + tests
- Parsers are the foundation — build and test them before any UI


## Glass Terminal Integration

Glass terminal history and context are available via MCP tools. Use `glass_history` to search past commands and output across sessions. Use `glass_context` for a summary of recent activity.
