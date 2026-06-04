use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ═══════════════ MemoryNodeId ═══════════════

pub type MemoryNodeId = String;

pub fn make_memory_id(content: &str, node_type: &str, timestamp: i64) -> MemoryNodeId {
    let input = format!("{}::{}::{}", content, node_type, timestamp);
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ═══════════════ 节点类型 ═══════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MemoryNodeType {
    Episode = 0,
    Bug = 1,
    Fix = 2,
    RootCause = 3,
    Insight = 4,
    Preference = 5,
}

impl MemoryNodeType {
    pub fn from_u8(n: u8) -> Option<Self> {
        match n {
            0 => Some(Self::Episode),
            1 => Some(Self::Bug),
            2 => Some(Self::Fix),
            3 => Some(Self::RootCause),
            4 => Some(Self::Insight),
            5 => Some(Self::Preference),
            _ => None,
        }
    }
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

// ═══════════════ 边类型 ═══════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MemoryEdgeType {
    CausedBy = 0,
    FixedBy = 1,
    SimilarTo = 2,
    LearnedFrom = 3,
    Contradicted = 4,
    Supersedes = 5,
    SupportedBy = 6,
    PrerequisiteOf = 7,
}

impl MemoryEdgeType {
    pub fn from_u8(n: u8) -> Option<Self> {
        match n {
            0 => Some(Self::CausedBy),
            1 => Some(Self::FixedBy),
            2 => Some(Self::SimilarTo),
            3 => Some(Self::LearnedFrom),
            4 => Some(Self::Contradicted),
            5 => Some(Self::Supersedes),
            6 => Some(Self::SupportedBy),
            7 => Some(Self::PrerequisiteOf),
            _ => None,
        }
    }
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

// ═══════════════ Episode ═══════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum EpisodeOutcome {
    Success = 0,
    Failure = 1,
    Partial = 2,
    Unknown = 3,
}

impl EpisodeOutcome {
    pub fn from_u8(n: u8) -> Option<Self> {
        match n {
            0 => Some(Self::Success),
            1 => Some(Self::Failure),
            2 => Some(Self::Partial),
            3 => Some(Self::Unknown),
            _ => None,
        }
    }
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: MemoryNodeId,
    pub session_id: String,
    pub user_request: String,
    pub agent_response: String,
    pub outcome: EpisodeOutcome,
    pub error_signature: Option<String>,
    pub tools_used: Vec<String>,
    pub files_modified: Vec<String>,
    pub token_usage: u64,
    pub duration_ms: u64,
    pub created_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

// ═══════════════ Bug ═══════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum BugSeverity {
    Critical = 0,
    High = 1,
    Medium = 2,
    Low = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bug {
    pub id: MemoryNodeId,
    pub description: String,
    pub stack_trace_hash: String,
    pub error_message: String,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
    pub severity: BugSeverity,
    pub occurrence_count: u32,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

// ═══════════════ Fix ═══════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fix {
    pub id: MemoryNodeId,
    pub description: String,
    pub strategy: FixStrategy,
    pub file_changes: Vec<FileChange>,
    pub verification_command: Option<String>,
    pub is_successful: bool,
    pub success_count: u32,
    pub failure_count: u32,
    pub created_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub file_path: String,
    pub old_string: String,
    pub new_string: String,
    pub change_type: ChangeType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ChangeType {
    Add = 0,
    Delete = 1,
    Modify = 2,
    Replace = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum FixStrategy {
    NullCheck = 0,
    TypeChange = 1,
    LogicFix = 2,
    DependencyUpdate = 3,
    ConfigChange = 4,
    Rewrite = 5,
    Other = 6,
}

// ═══════════════ RootCause ═══════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCause {
    pub id: MemoryNodeId,
    pub description: String,
    pub category: RootCauseCategory,
    pub confidence: f32,
    pub supporting_evidence_count: u32,
    pub identified_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum RootCauseCategory {
    MissingNullCheck = 0,
    IncorrectType = 1,
    LogicError = 2,
    ApiMisuse = 3,
    ConcurrencyBug = 4,
    ResourceLeak = 5,
    ConfigurationError = 6,
    DependencyIssue = 7,
    Unknown = 8,
}

// ═══════════════ Insight ═══════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum InsightStatus {
    Exploration = 0,
    Stable = 1,
    Conflicting = 2,
    Deprecated = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    pub id: MemoryNodeId,
    pub content: String,
    pub confidence: f32,
    pub version: u32,
    pub source_count: u32,
    pub utility_score: f32,
    pub last_activated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub status: InsightStatus,
    pub metadata: serde_json::Value,
}

// ═══════════════ 检索结果 ═══════════════

#[derive(Debug, Clone)]
pub struct RetrievedMemory {
    pub node_id: MemoryNodeId,
    pub node_type: MemoryNodeType,
    pub content: String,
    pub score: f32,
    pub semantic_score: f32,
    pub graph_score: f32,
    pub source_episode_ids: Vec<MemoryNodeId>,
    pub confidence: f32,
    pub last_updated: DateTime<Utc>,
}

// ═══════════════ 门控与巩固 ═══════════════

#[derive(Debug, Clone)]
pub struct GateInput {
    pub task_description: String,
    pub memory_content: String,
    pub memory_type: MemoryNodeType,
    pub occurrence_count: u32,
    pub cross_session_count: u32,
    pub corrective: bool,
}

#[derive(Debug, Clone)]
pub struct GateResult {
    pub admitted: bool,
    pub utility_score: f32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsolidationState {
    Idle,
    Running { started_at: DateTime<Utc> },
    Completed { insights_generated: u32, pruned_count: u32 },
    Failed { error: String },
}

// ═══════════════ 统一节点包装 ═══════════════

#[derive(Debug, Clone)]
pub enum MemoryNode {
    Episode(Episode),
    Bug(Bug),
    Fix(Fix),
    RootCause(RootCause),
    Insight(Insight),
    Preference(Preference),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preference {
    pub id: MemoryNodeId,
    pub key: String,           // e.g. "naming", "error_handling"
    pub value: String,         // e.g. "use snake_case", "prefer Result over panic!"
    pub evidence_count: u32,   // how many times reinforced
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub confidence: f32,
}

impl MemoryNode {
    pub fn node_type(&self) -> MemoryNodeType {
        match self {
            Self::Episode(_) => MemoryNodeType::Episode,
            Self::Bug(_) => MemoryNodeType::Bug,
            Self::Fix(_) => MemoryNodeType::Fix,
            Self::RootCause(_) => MemoryNodeType::RootCause,
            Self::Insight(_) => MemoryNodeType::Insight,
            Self::Preference(_) => MemoryNodeType::Preference,
        }
    }

    pub fn id(&self) -> &MemoryNodeId {
        match self {
            Self::Episode(e) => &e.id,
            Self::Bug(b) => &b.id,
            Self::Fix(f) => &f.id,
            Self::RootCause(r) => &r.id,
            Self::Insight(i) => &i.id,
            Self::Preference(p) => &p.id,
        }
    }

    pub fn content_summary(&self) -> String {
        match self {
            Self::Episode(e) => format!(
                "[Episode] {} | outcome={:?} | {}",
                &e.user_request.chars().take(200).collect::<String>(),
                e.outcome,
                e.created_at
            ),
            Self::Bug(b) => format!(
                "[Bug] {} | severity={:?} | count={}",
                b.description, b.severity, b.occurrence_count
            ),
            Self::Fix(f) => format!(
                "[Fix] {} | strategy={:?} | success={}/{}",
                f.description, f.strategy, f.success_count, f.failure_count
            ),
            Self::RootCause(r) => format!(
                "[RootCause] {} | category={:?} | confidence={:.2}",
                r.description, r.category, r.confidence
            ),
            Self::Insight(i) => format!(
                "[Insight] {} | confidence={:.2} | utility={:.2} | version={}",
                i.content, i.confidence, i.utility_score, i.version
            ),
            Self::Preference(p) => format!(
                "[Preference] {}={} | confidence={:.2} | evidence={}",
                p.key, p.value, p.confidence, p.evidence_count
            ),
        }
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        match self {
            Self::Episode(e) => e.created_at,
            Self::Bug(b) => b.first_seen_at,
            Self::Fix(f) => f.created_at,
            Self::RootCause(r) => r.identified_at,
            Self::Insight(i) => i.created_at,
            Self::Preference(p) => p.last_seen,
        }
    }
}

// ═══════════════ tests ═══════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_roundtrip() {
        for nt in [
            MemoryNodeType::Episode,
            MemoryNodeType::Bug,
            MemoryNodeType::Fix,
            MemoryNodeType::RootCause,
            MemoryNodeType::Insight,
        ] {
            assert_eq!(MemoryNodeType::from_u8(nt.to_u8()), Some(nt));
        }
        assert_eq!(MemoryNodeType::from_u8(255), None);
    }

    #[test]
    fn test_edge_type_roundtrip() {
        for et in [
            MemoryEdgeType::CausedBy,
            MemoryEdgeType::FixedBy,
            MemoryEdgeType::SimilarTo,
            MemoryEdgeType::LearnedFrom,
            MemoryEdgeType::Contradicted,
            MemoryEdgeType::Supersedes,
            MemoryEdgeType::SupportedBy,
            MemoryEdgeType::PrerequisiteOf,
        ] {
            assert_eq!(MemoryEdgeType::from_u8(et.to_u8()), Some(et));
        }
        assert_eq!(MemoryEdgeType::from_u8(255), None);
    }

    #[test]
    fn test_memory_id_deterministic() {
        let id1 = make_memory_id("test bug", "Bug", 12345);
        let id2 = make_memory_id("test bug", "Bug", 12345);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 64);
    }

    #[test]
    fn test_episode_serde_roundtrip() {
        let ep = Episode {
            id: make_memory_id("req", "Episode", 1),
            session_id: "s1".into(),
            user_request: "fix bug".into(),
            agent_response: "done".into(),
            outcome: EpisodeOutcome::Success,
            error_signature: None,
            tools_used: vec!["bash".into()],
            files_modified: vec!["src/main.rs".into()],
            token_usage: 1000,
            duration_ms: 5000,
            created_at: Utc::now(),
            metadata: serde_json::json!({}),
        };
        let json = serde_json::to_string(&ep).unwrap();
        let back: Episode = serde_json::from_str(&json).unwrap();
        assert_eq!(back.user_request, "fix bug");
    }

    #[test]
    fn test_content_summary() {
        let bug = MemoryNode::Bug(Bug {
            id: "id".into(),
            description: "null pointer".into(),
            stack_trace_hash: "abc".into(),
            error_message: "NPE at line 5".into(),
            file_path: Some("src/lib.rs".into()),
            line_number: Some(5),
            severity: BugSeverity::High,
            occurrence_count: 3,
            first_seen_at: Utc::now(),
            last_seen_at: Utc::now(),
            metadata: serde_json::json!({}),
        });
        let summary = bug.content_summary();
        assert!(summary.contains("null pointer"));
        assert!(summary.contains("Bug"));
    }
}
