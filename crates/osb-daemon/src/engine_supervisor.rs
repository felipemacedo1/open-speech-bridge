//! Engine lifecycle management.

use osb_protocol::engine::{EngineId, EngineInfo, EngineState};
use std::collections::HashMap;
use tracing::info;

/// Manages engine lifecycle and health.
pub struct EngineSupervisor {
    engines: HashMap<EngineId, EngineEntry>,
}

struct EngineEntry {
    info: EngineInfo,
    state: EngineState,
}

impl EngineSupervisor {
    /// Create a new engine supervisor.
    pub fn new() -> Self {
        Self {
            engines: HashMap::new(),
        }
    }

    /// Register an engine.
    pub fn register(&mut self, info: EngineInfo) {
        let id = info.id.clone();
        info!(engine_id = %id, name = %info.name, "registering engine");

        self.engines.insert(
            id,
            EngineEntry {
                info,
                state: EngineState::Stopped,
            },
        );
    }

    /// Get engine info by ID.
    pub fn get_info(&self, id: &EngineId) -> Option<&EngineInfo> {
        self.engines.get(id).map(|e| &e.info)
    }

    /// Get engine state by ID.
    pub fn get_state(&self, id: &EngineId) -> Option<EngineState> {
        self.engines.get(id).map(|e| e.state)
    }

    /// List all registered engines.
    pub fn list_engines(&self) -> Vec<&EngineInfo> {
        self.engines.values().map(|e| &e.info).collect()
    }

    /// Start an engine.
    pub fn start_engine(&mut self, id: &EngineId) -> Result<(), String> {
        let entry = self.engines.get_mut(id).ok_or("engine not found")?;

        if entry.state != EngineState::Stopped {
            return Err("engine is not stopped".to_string());
        }

        info!(engine_id = %id, "starting engine");
        entry.state = EngineState::Starting;

        // In a real implementation, we would:
        // 1. Spawn the engine process (if external)
        // 2. Send initialization message
        // 3. Wait for ready signal
        // 4. Update state to Ready

        entry.state = EngineState::Ready;
        Ok(())
    }

    /// Stop an engine.
    pub fn stop_engine(&mut self, id: &EngineId) -> Result<(), String> {
        let entry = self.engines.get_mut(id).ok_or("engine not found")?;

        info!(engine_id = %id, "stopping engine");
        entry.state = EngineState::ShuttingDown;

        // Graceful shutdown

        entry.state = EngineState::Stopped;
        Ok(())
    }

    /// Find engines with specific capabilities.
    pub fn find_by_capability(
        &self,
        capabilities: &[osb_protocol::capability::Capability],
    ) -> Vec<&EngineInfo> {
        self.engines
            .values()
            .filter(|e| e.info.capabilities.has_all(capabilities))
            .map(|e| &e.info)
            .collect()
    }

    /// Get the count of running engines.
    pub fn running_count(&self) -> usize {
        self.engines
            .values()
            .filter(|e| e.state == EngineState::Ready || e.state == EngineState::Processing)
            .count()
    }
}

impl Default for EngineSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osb_protocol::capability::{Capability, CapabilitySet};
    use osb_protocol::engine::EngineType;

    #[test]
    fn test_engine_registration() {
        let mut supervisor = EngineSupervisor::new();

        let mut caps = CapabilitySet::new();
        caps.add(Capability::StreamingStt);

        let info = EngineInfo::builder("test-engine")
            .name("Test Engine")
            .version("1.0.0")
            .engine_type(EngineType::Mock)
            .capabilities(caps)
            .build();

        supervisor.register(info);

        assert!(supervisor.get_info(&EngineId::new("test-engine")).is_some());
        assert_eq!(
            supervisor.get_state(&EngineId::new("test-engine")),
            Some(EngineState::Stopped)
        );
    }

    #[test]
    fn test_engine_lifecycle() {
        let mut supervisor = EngineSupervisor::new();

        let info = EngineInfo::builder("test").name("Test").build();

        supervisor.register(info);
        let id = EngineId::new("test");

        assert_eq!(supervisor.get_state(&id), Some(EngineState::Stopped));

        supervisor.start_engine(&id).unwrap();
        assert_eq!(supervisor.get_state(&id), Some(EngineState::Ready));

        supervisor.stop_engine(&id).unwrap();
        assert_eq!(supervisor.get_state(&id), Some(EngineState::Stopped));
    }
}
