# Rules: Local Documentation & Memory Bank (.kiro) Maintenance

These rules govern how the agent interacts with and maintains the persistent `.kiro/` documentation.

## Core Mandates

1. **Read-First Principle**:
   - At the beginning of any development session or complex task, read [`.kiro/steering/01-memory-bank.md`](.kiro/steering/01-memory-bank.md) to understand:
     - The current phase of development (e.g., M1 Audio Bridge).
     - Completed tasks vs tasks in progress.
     - Known blockers or pending items.
     - Recent changes history.

2. **Continuous Evolution**:
   - Whenever you complete a feature, fix a bug, refactor code, or add new tests, you **MUST** update `01-memory-bank.md`.
   - Update the following sections:
     - **Estado Atual do Desenvolvimento**: Mark completed tasks with `[x]`, add new tasks in progress.
     - **Histórico de Mudanças Recentes**: Add a row with Date, Change summary, and Affected files.
     - **Status do Build & Testes**: Update test count (e.g., `cargo test` passing count), clippy status, and warnings count.
     - **Próximos Passos**: Re-prioritize the next 3-5 immediate technical actions.

3. **Project Context Synchronization**:
   - If crates are added/removed, dependencies change, or architecture is updated, update [`.kiro/steering/00-project-context.md`](.kiro/steering/00-project-context.md).

4. **Preservation**:
   - Never wipe or overwrite `.kiro/` without preserving historical memory.
   - Respect developer preferences recorded in `01-memory-bank.md`:
     - Language for code and commits: English.
     - Language for user communication: Brazilian Portuguese.
     - Invariants: Real-time audio safety, no non-commercial models by default.
