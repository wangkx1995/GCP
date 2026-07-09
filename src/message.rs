use crate::core_agent_api::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InternalMessage {
    AgentRegister(AgentRegisterRequest),
    AgentRegisterAck(AgentRegisterResponse),
    Heartbeat(AgentHeartbeatRequest),
    HeartbeatAck,

    DispatchTask(TaskDispatchRequest),
    DispatchTaskAck(TaskDispatchResponse),

    TaskEvent(TaskEventRequest),
    TaskResult(TaskResultReport),

    ConfigSnapshotRequest(String),
    ConfigSnapshotResponse(ConfigSnapshotResponse),

    CancelTask(String),
    AgentDisconnected,
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internal_message_roundtrip() {
        let msg = InternalMessage::HeartbeatAck;
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: InternalMessage = bincode::deserialize(&bytes).unwrap();
        assert!(matches!(decoded, InternalMessage::HeartbeatAck));

        let msg = InternalMessage::Shutdown;
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: InternalMessage = bincode::deserialize(&bytes).unwrap();
        assert!(matches!(decoded, InternalMessage::Shutdown));

        let task_result = TaskResultReport {
            task_id: "test-task".into(),
            agent_id: "agent-01".into(),
            status: TaskStatus::Succeeded,
            result_rows: vec![CsvResultRow {
                table_name: "TPD_A".into(),
                data_time: "2026-06-17 15:15:00".into(),
                row_count: 100,
                success: 1,
                collect_time: "2026-07-02 15:35:00".into(),
                task_id: "test-task".into(),
                strategy_id: "".into(),
                group_id: "".into(),
            }],
        };
        let msg = InternalMessage::TaskResult(task_result.clone());
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: InternalMessage = bincode::deserialize(&bytes).unwrap();
        match decoded {
            InternalMessage::TaskResult(report) => {
                assert_eq!(report.task_id, "test-task");
                assert_eq!(report.result_rows.len(), 1);
                assert_eq!(report.result_rows[0].row_count, 100);
            }
            _ => panic!("expected TaskResult"),
        }
    }
}
