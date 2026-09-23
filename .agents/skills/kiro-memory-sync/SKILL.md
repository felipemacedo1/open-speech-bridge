---
name: kiro-memory-sync
description: >-
  Use this skill whenever you need to read, update, synchronize, or evolve the local project documentation and Memory Bank in .kiro/ (especially 01-memory-bank.md, 00-project-context.md, and docs/STATUS.md).
---

# Skill: Memory Bank (.kiro) Synchronization & Maintenance

This skill guides the agent in keeping local memory, context, and project documentation up to date across sessions.

---

## 1. When to Use

Execute this procedure:
1. **At the start of a session**: Read `.kiro/steering/01-memory-bank.md` to establish current state and identify pending tasks.
2. **At the completion of a task / feature**: Update `.kiro/steering/01-memory-bank.md` with achievements, test results, and next actions.
3. **When architectural decisions change**: Update `.kiro/steering/00-project-context.md` and `docs/STATUS.md`.

---

## 2. Reading Context (Start of Session)

Check the following sections in [`01-memory-bank.md`](.kiro/steering/01-memory-bank.md):
- **Estado Atual do Desenvolvimento**: What was the focus of the last session?
- **Em Progresso / Bloqueios**: Any active blockers (e.g. Docker status, auth, compilation errors)?
- **Próximos Passos**: What are the top 3 priorities?

---

## 3. Updating the Memory Bank (End of Session / Post-Task)

Edit [`01-memory-bank.md`](.kiro/steering/01-memory-bank.md) with the following structure:

### 1. Update "Estado Atual do Desenvolvimento"
```markdown
### Última Sessão
- **Data**: YYYY-MM-DD
- **Foco**: [Ex: Implementação de enumeração de dispositivos PipeWire]
- **Status**: ✅ [Resumo do status, ex: 105 testes passando, zero warnings]

### Em Progresso
- [x] [Tarefas concluídas]
- [ ] [Tarefas atualmente em andamento]
```

### 2. Append to "Histórico de Mudanças Recentes"
Add a new row to the table:
```markdown
| YYYY-MM-DD | <Descrição da mudança> | <Arquivos afetados> |
```

### 3. Update "Status do Build"
Record the exact results from Docker:
- `cargo test`: Number of passed unit tests and doc-tests.
- `cargo clippy`: Status (e.g. zero warnings).
- `cargo doc` / `cargo fmt`: Status.

### 4. Update "Próximos Passos"
List 3-5 immediate, actionable technical steps for the next turn or session.

---

## 4. Keeping docs/STATUS.md in Sync

Whenever milestone items change:
1. Update checkboxes in [`docs/STATUS.md`](docs/STATUS.md).
2. Move items from "What's In Progress" to "What Works Today" once fully implemented and tested.
