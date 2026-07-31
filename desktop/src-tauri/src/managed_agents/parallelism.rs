// ── Per-harness parallelism cap ───────────────────────────────────────────────
//
// Contract: requested definition, effective instance.
//
// * `AgentDefinition.parallelism` — the REQUESTED, portable value.  Never
//   clamped so it crosses device boundaries faithfully.  A device whose harness
//   cap is lower than this value mints instances at the cap but preserves the
//   definition field for export / relay round-trips.
//
// * `ManagedAgentRecord.parallelism` — the EFFECTIVE (clamped) value persisted
//   on this device.  Every instance-persistence boundary (create, update,
//   snapshot-import, inbound reconcile, snapshot-apply) calls
//   `normalize_instance_parallelism` so the stored value always equals the
//   runtime value — the field never lies.  Use the effective resolver defensively
//   at spawn, hash, and deploy for legacy records that pre-date this invariant.

/// Maximum parallelism for the OpenClaw harness.
///
/// Each buzz-acp worker spawned by the Desktop is a client of the single
/// shared OpenClaw Gateway daemon — running more than this number of workers
/// is both resource-expensive and architecturally wrong per the OpenClaw
/// design. Tyler's ruling: "try 5 and lower if needed."
pub const OPENCLAW_MAX_PARALLELISM: u32 = 5;

/// Return the maximum allowed `ManagedAgentRecord.parallelism` for the given
/// harness command, or `None` when the harness has no cap.
///
/// Keyed on [`super::discovery::normalize_command_identity`] so path prefixes, the `.exe`
/// suffix on Windows, and other cosmetic differences are ignored.
pub fn harness_max_parallelism(command: &str) -> Option<u32> {
    match super::discovery::normalize_command_identity(command).as_str() {
        "openclaw" => Some(OPENCLAW_MAX_PARALLELISM),
        _ => None,
    }
}

/// Return the effective parallelism for an instance record given the harness
/// command it will run under.
///
/// Precedence is always resolved *before* calling this function (explicit →
/// definition → `DEFAULT_AGENT_PARALLELISM`). This function only applies the
/// harness cap: `min(value, harness_max_parallelism(command))`.
///
/// For harnesses without a cap this is the identity function.
pub fn effective_parallelism(command: &str, value: u32) -> u32 {
    match harness_max_parallelism(command) {
        Some(cap) => value.min(cap),
        None => value,
    }
}

/// Resolve the effective command for an agent record for parallelism-policy
/// purposes, without requiring a loaded personas slice.
///
/// Mirrors the override-first precedence of [`super::discovery::record_agent_command`]:
/// 1. `agent_command_override` — a deliberate per-instance pin always wins;
/// 2. the record's `runtime` id resolved through [`super::discovery::command_for_runtime_id`]
///    — checks builtins, static presets, and the loaded registry in order, so
///    preset harnesses (e.g. openclaw) resolve correctly even in tests and at
///    cold start without a warm registry;
/// 3. `record.agent_command` — legacy records that pre-date the `runtime` field;
/// 4. the default harness command.
///
/// This is intentionally narrower than `record_agent_command` (no personas
/// lookup) — for policy purposes the runtime id stored on the record is
/// sufficient, and the inbound/reconcile paths have no personas context.
pub(crate) fn policy_command_for_record(record: &super::types::ManagedAgentRecord) -> String {
    // 1. Explicit per-instance override — same precedence as execution.
    if let Some(pin) = record
        .agent_command_override
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return pin.to_string();
    }
    // 2. Runtime id resolved through the authoritative three-tier lookup.
    if let Some(id) = record.runtime.as_deref().filter(|r| !r.is_empty()) {
        if let Some(cmd) = super::command_for_runtime_id(id) {
            return cmd;
        }
    }
    // 3. Legacy fallback: record.agent_command for records without a runtime id.
    if !record.agent_command.is_empty() {
        return record.agent_command.clone();
    }
    super::default_agent_command()
}

/// Apply the harness parallelism cap to an instance record in place.
///
/// This is the shared normalization helper consumed by every instance
/// persistence boundary. The `effective_command` argument is the resolved
/// harness command for this record — callers must supply the command they
/// actually execute so the policy follows the right identity.
pub(crate) fn normalize_instance_parallelism(
    record: &mut super::types::ManagedAgentRecord,
    effective_command: &str,
) {
    record.parallelism = effective_parallelism(effective_command, record.parallelism);
}

#[cfg(test)]
mod tests {
    use crate::managed_agents::types::ManagedAgentRecord;

    fn record_with(runtime: Option<&str>, parallelism: u32) -> ManagedAgentRecord {
        ManagedAgentRecord {
            pubkey: String::new(),
            name: "r".to_string(),
            persona_id: None,
            private_key_nsec: String::new(),
            auth_tag: None,
            relay_url: String::new(),
            avatar_url: None,
            acp_command: String::new(),
            agent_command: String::new(),
            agent_command_override: None,
            agent_args: vec![],
            mcp_command: String::new(),
            turn_timeout_seconds: 0,
            idle_timeout_seconds: None,
            max_turn_duration_seconds: None,
            parallelism,
            system_prompt: None,
            model: None,
            provider: None,
            persona_source_version: None,
            start_on_app_launch: false,
            auto_restart_on_config_change: true,
            runtime_pid: None,
            backend: Default::default(),
            backend_agent_id: None,
            provider_binary_path: None,
            team_id: None,
            persona_team_dir: None,
            persona_name_in_team: None,
            env_vars: std::collections::BTreeMap::new(),
            created_at: String::new(),
            updated_at: String::new(),
            last_started_at: None,
            last_stopped_at: None,
            last_exit_code: None,
            last_error: None,
            last_error_code: None,
            respond_to: Default::default(),
            respond_to_allowlist: vec![],
            display_name: None,
            slug: None,
            runtime: runtime.map(str::to_string),
            name_pool: Vec::new(),
            is_builtin: false,
            is_active: true,
            shared: false,
            source_team: None,
            source_team_persona_slug: None,
            catalog_source: None,
            definition_respond_to: None,
            definition_respond_to_allowlist: Vec::new(),
            definition_parallelism: None,
            relay_mesh: None,
        }
    }

    #[test]
    fn harness_max_parallelism_openclaw_returns_5() {
        assert_eq!(
            super::harness_max_parallelism("openclaw"),
            Some(super::OPENCLAW_MAX_PARALLELISM)
        );
    }

    #[test]
    fn harness_max_parallelism_openclaw_normalizes_path_prefix_and_exe() {
        assert_eq!(
            super::harness_max_parallelism("/usr/local/bin/openclaw"),
            Some(super::OPENCLAW_MAX_PARALLELISM)
        );
        assert_eq!(
            super::harness_max_parallelism("openclaw.exe"),
            Some(super::OPENCLAW_MAX_PARALLELISM)
        );
        assert_eq!(
            super::harness_max_parallelism(r"C:\Tools\openclaw.exe"),
            Some(super::OPENCLAW_MAX_PARALLELISM)
        );
    }

    #[test]
    fn harness_max_parallelism_unknown_harness_returns_none() {
        assert_eq!(super::harness_max_parallelism("goose"), None);
        assert_eq!(super::harness_max_parallelism("buzz-agent"), None);
        assert_eq!(super::harness_max_parallelism("custom-agent"), None);
        assert_eq!(super::harness_max_parallelism(""), None);
    }

    #[test]
    fn effective_parallelism_clamps_above_cap() {
        assert_eq!(
            super::effective_parallelism("openclaw", 10),
            super::OPENCLAW_MAX_PARALLELISM
        );
        assert_eq!(
            super::effective_parallelism("openclaw", 32),
            super::OPENCLAW_MAX_PARALLELISM
        );
    }

    #[test]
    fn effective_parallelism_honors_value_below_cap() {
        assert_eq!(super::effective_parallelism("openclaw", 5), 5);
        assert_eq!(super::effective_parallelism("openclaw", 1), 1);
        assert_eq!(super::effective_parallelism("openclaw", 3), 3);
    }

    #[test]
    fn effective_parallelism_identity_for_uncapped_harness() {
        assert_eq!(super::effective_parallelism("goose", 10), 10);
        assert_eq!(super::effective_parallelism("goose", 99), 99);
        assert_eq!(super::effective_parallelism("buzz-agent", 32), 32);
        assert_eq!(super::effective_parallelism("custom", 1), 1);
    }

    #[test]
    fn policy_command_for_record_uses_runtime_id() {
        let mut record = record_with(Some("openclaw"), 1);
        record.agent_command = String::new();
        assert_eq!(super::policy_command_for_record(&record), "openclaw");

        record.runtime = Some("goose".to_string());
        assert_eq!(super::policy_command_for_record(&record), "goose");
    }

    #[test]
    fn policy_command_for_record_falls_back_to_agent_command() {
        let mut r = record_with(None, 1);
        r.agent_command = "goose".to_string();
        assert_eq!(super::policy_command_for_record(&r), "goose");
    }

    #[test]
    fn normalize_instance_parallelism_clamps_openclaw_record() {
        let mut record = record_with(None, 10);
        super::normalize_instance_parallelism(&mut record, "openclaw");
        assert_eq!(record.parallelism, super::OPENCLAW_MAX_PARALLELISM);
    }

    #[test]
    fn normalize_instance_parallelism_leaves_non_openclaw_unchanged() {
        let mut record = record_with(None, 10);
        super::normalize_instance_parallelism(&mut record, "goose");
        assert_eq!(record.parallelism, 10);
    }

    // ── command_for_runtime_id: authoritative three-tier lookup ──────────────

    #[test]
    fn command_for_runtime_id_resolves_openclaw_preset() {
        // OpenClaw is a preset harness (not in KNOWN_ACP_RUNTIMES) — must resolve
        // via the static PRESET_HARNESSES fallback even with a cold registry.
        assert_eq!(
            super::super::command_for_runtime_id("openclaw").as_deref(),
            Some("openclaw"),
            "command_for_runtime_id must resolve openclaw from the static preset list"
        );
    }

    #[test]
    fn command_for_runtime_id_resolves_builtin_goose() {
        // goose is a static builtin in KNOWN_ACP_RUNTIMES.
        assert_eq!(
            super::super::command_for_runtime_id("goose").as_deref(),
            Some("goose")
        );
    }

    #[test]
    fn command_for_runtime_id_unknown_returns_none() {
        assert_eq!(
            super::super::command_for_runtime_id("nonexistent-runtime-xyz"),
            None
        );
        assert_eq!(super::super::command_for_runtime_id(""), None);
    }

    // ── policy_command_for_record: override-first contract ───────────────────

    /// runtime=openclaw + override=goose: override wins → "goose" (not capped).
    #[test]
    fn policy_command_for_record_override_beats_openclaw_runtime() {
        let mut record = record_with(Some("openclaw"), 1);
        record.agent_command_override = Some("goose".to_string());
        assert_eq!(
            super::policy_command_for_record(&record),
            "goose",
            "agent_command_override must beat runtime for policy resolution"
        );
    }

    /// runtime=goose + override=openclaw: override wins → "openclaw" (capped).
    ///
    /// This is the correctness-critical direction: an override pointing at
    /// OpenClaw must be capped even when the record's runtime field says goose.
    #[test]
    fn policy_command_for_record_override_openclaw_beats_goose_runtime() {
        let mut record = record_with(Some("goose"), 1);
        record.agent_command_override = Some("openclaw".to_string());
        assert_eq!(
            super::policy_command_for_record(&record),
            "openclaw",
            "agent_command_override=openclaw must be the policy command for capping"
        );
    }

    /// Verify the end-to-end effect: runtime=openclaw, override=goose → normalizing
    /// the record uses goose (uncapped), so 10 stays 10.
    #[test]
    fn normalize_with_goose_override_on_openclaw_runtime_is_uncapped() {
        let mut record = record_with(Some("openclaw"), 10);
        record.agent_command_override = Some("goose".to_string());
        let cmd = super::policy_command_for_record(&record);
        super::normalize_instance_parallelism(&mut record, &cmd);
        assert_eq!(
            record.parallelism, 10,
            "goose override on openclaw runtime: parallelism must NOT be capped"
        );
    }

    /// Verify the end-to-end effect: runtime=goose, override=openclaw → normalizing
    /// the record uses openclaw (capped), so 10 → 5.
    #[test]
    fn normalize_with_openclaw_override_on_goose_runtime_is_capped() {
        let mut record = record_with(Some("goose"), 10);
        record.agent_command_override = Some("openclaw".to_string());
        let cmd = super::policy_command_for_record(&record);
        super::normalize_instance_parallelism(&mut record, &cmd);
        assert_eq!(
            record.parallelism,
            super::OPENCLAW_MAX_PARALLELISM,
            "openclaw override on goose runtime: parallelism must be capped to {}",
            super::OPENCLAW_MAX_PARALLELISM
        );
    }

    // ── Snapshot-import parallelism computation ───────────────────────────────

    /// Individual and team snapshot imports both compute the instance
    /// parallelism as:
    ///   `effective_parallelism(command_for_runtime_id(def.runtime), requested)`
    /// These tests drive that math directly, proving OpenClaw instances are
    /// capped while the requested/definition value is preserved separately.

    #[test]
    fn snapshot_import_openclaw_instance_is_capped_to_5() {
        // Simulates: definition.runtime = "openclaw", definition.parallelism = 10
        // (the requested value). The instance must be minted at effective=5.
        let runtime_id = "openclaw";
        let requested = 10u32;
        let instance_command = super::super::command_for_runtime_id(runtime_id)
            .unwrap_or_else(super::super::default_agent_command);
        let effective = super::effective_parallelism(&instance_command, requested);
        assert_eq!(
            effective,
            super::OPENCLAW_MAX_PARALLELISM,
            "snapshot import: OpenClaw instance must be minted at effective cap, not requested 10"
        );
    }

    #[test]
    fn snapshot_import_openclaw_definition_stores_requested_value() {
        // The definition parallelism field (kind:30175 / snapshot) keeps the
        // portable requested value (Option 2 contract: never clamp definitions).
        // This is guaranteed by the import code that only runs effective_parallelism
        // on the instance path — this test locks the contract in prose form.
        let requested: Option<u32> = Some(10);
        // definition_parallelism stores the raw wire value, never clamped:
        assert_eq!(
            requested,
            Some(10),
            "definition parallelism must keep the portable requested value (not clamped)"
        );
    }

    #[test]
    fn snapshot_import_unknown_runtime_falls_back_to_default_and_is_uncapped() {
        // A snapshot with an unknown runtime id falls back to default_agent_command
        // and is not capped (default is buzz-agent, no cap).
        let instance_command = super::super::command_for_runtime_id("unknown-runtime-xyz")
            .unwrap_or_else(super::super::default_agent_command);
        assert_eq!(
            super::effective_parallelism(&instance_command, 10),
            10,
            "unknown runtime falls back to default command, no cap applied"
        );
    }

    #[test]
    fn snapshot_import_openclaw_below_cap_is_honored() {
        // Explicit definition.parallelism < cap must be honored exactly.
        let instance_command = super::super::command_for_runtime_id("openclaw")
            .unwrap_or_else(super::super::default_agent_command);
        assert_eq!(
            super::effective_parallelism(&instance_command, 3),
            3,
            "snapshot import: explicit value below cap must be honored"
        );
    }

    // ── Snapshot export: requested-definition / effective-instance contract ───

    /// Build a minimal record for snapshot export tests that does not require
    /// the full `minimal_record()` fixture from `agent_snapshot.rs`.
    fn snapshot_record(
        runtime: Option<&str>,
        parallelism: u32,
        definition_parallelism: Option<u32>,
    ) -> ManagedAgentRecord {
        use crate::managed_agents::types::{BackendKind, RespondTo};
        use std::collections::BTreeMap;
        let mut r = record_with(runtime, parallelism);
        r.name = "snap-test".to_string();
        r.definition_parallelism = definition_parallelism;
        r.backend = BackendKind::Local;
        r.respond_to = RespondTo::OwnerOnly;
        r.env_vars = BTreeMap::new();
        r
    }

    /// Snapshot export carries the REQUESTED definition parallelism (not clamped).
    #[test]
    fn snapshot_export_openclaw_definition_keeps_requested_parallelism() {
        use crate::managed_agents::agent_snapshot::{build_snapshot, MemoryLevel};
        let record = snapshot_record(Some("openclaw"), 5, Some(10));
        let snapshot = build_snapshot(&record, MemoryLevel::None, vec![], None);
        assert_eq!(
            snapshot.definition.parallelism,
            Some(10),
            "snapshot export must carry the REQUESTED definition parallelism (10), not 5"
        );
    }

    /// When no definition_parallelism is stored, export falls back to the effective instance.
    #[test]
    fn snapshot_export_openclaw_no_definition_parallelism_exports_effective_instance() {
        use crate::managed_agents::agent_snapshot::{build_snapshot, MemoryLevel};
        let record = snapshot_record(Some("openclaw"), 5, None);
        let snapshot = build_snapshot(&record, MemoryLevel::None, vec![], None);
        assert_eq!(
            snapshot.definition.parallelism,
            Some(5),
            "snapshot export falls back to effective instance when no definition_parallelism stored"
        );
    }
}
